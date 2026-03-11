use crate::APP_USER_AGENT;
use crate::config::DatasetRepoConfig;
use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::BodyExt;
use octocrab::Octocrab;
use octocrab::params::repos::Reference;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tracing::instrument;
use vacs_vatsim::coverage::network::Network;

/// File name used to track the currently deployed commit SHA.
const VERSION_FILE: &str = ".dataset-sha";

/// Subset of the GitHub Git Tags API response that includes the target object.
///
/// The upstream `octocrab::models::repos::GitTag` omits the `object` field required
/// to resolve an annotated tag to its commit SHA.
#[derive(Debug, Deserialize)]
struct GitTagWithObject {
    object: GitTagTarget,
}

#[derive(Debug, Deserialize)]
struct GitTagTarget {
    sha: String,
    r#type: String,
}

/// Manages downloading, validating, and installing dataset updates from GitHub.
pub struct DatasetManager {
    octocrab: Octocrab,
    owner: String,
    repo: String,
    deployed_tag: String,
    coverage_dir: PathBuf,
}

impl DatasetManager {
    /// Create a new `DatasetManager`.
    ///
    /// When `credentials` are provided, the client authenticates as a GitHub
    /// App installation. Otherwise it falls back to unauthenticated requests
    /// (only works for public repositories).
    pub async fn new(config: &DatasetRepoConfig, coverage_dir: impl Into<PathBuf>) -> Result<Self> {
        let mut client_builder =
            Octocrab::builder().add_header(reqwest::header::USER_AGENT, APP_USER_AGENT.to_string());

        let octocrab = if let Some(credentials) = &config.credentials {
            tracing::info!(
                ?credentials,
                "Using GitHub App authentication for dataset repository"
            );

            let key =
                jsonwebtoken::EncodingKey::from_rsa_pem(credentials.app_private_key.as_bytes())
                    .context(
                        "Failed to parse GitHub App private key for dataset repository access",
                    )?;

            client_builder = client_builder.app(credentials.app_id.into(), key);

            client_builder
                .build()
                .context("Failed to build Octocrab client for dataset repository")?
                .installation(credentials.installation_id.into())
                .context("Failed to set GitHub App installation ID")?
        } else {
            tracing::info!("Using unauthenticated GitHub access for dataset repository");
            client_builder
                .build()
                .context("Failed to build Octocrab client for dataset repository")?
        };

        Ok(Self {
            octocrab,
            owner: config.owner.clone(),
            repo: config.repo.clone(),
            deployed_tag: config.deployed_tag.clone(),
            coverage_dir: coverage_dir.into(),
        })
    }

    /// Get the locally stored commit SHA, if any.
    pub fn local_sha(&self) -> Option<String> {
        let path = self.coverage_dir.join(VERSION_FILE);
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Save the deployed commit SHA to disk.
    fn save_sha(&self, sha: &str) -> Result<()> {
        let path = self.coverage_dir.join(VERSION_FILE);
        std::fs::write(&path, sha)
            .with_context(|| format!("Failed to write version file at {path:?}"))
    }

    /// Resolve the `deployed/<env>` tag to its commit SHA.
    #[instrument(level = "info", skip(self), fields(owner = %self.owner, repo = %self.repo, tag = %self.deployed_tag))]
    pub async fn resolve_deployed_sha(&self) -> Result<String> {
        let git_ref = self
            .octocrab
            .repos(&self.owner, &self.repo)
            .get_ref(&Reference::Tag(self.deployed_tag.clone()))
            .await
            .with_context(|| {
                format!(
                    "Failed to resolve tag '{}', might be missing in repository",
                    self.deployed_tag
                )
            })?;

        let sha = match git_ref.object {
            octocrab::models::repos::Object::Commit { sha, .. } => sha,
            octocrab::models::repos::Object::Tag { sha, .. } => {
                // Annotated tag - the SHA here is the tag object, not the actual commit
                self.resolve_tag_to_commit_sha(&sha).await?
            }
            other => {
                anyhow::bail!(
                    "Unexpected git object type for tag '{}': {other:?}",
                    self.deployed_tag
                );
            }
        };

        tracing::info!(%sha, tag = %self.deployed_tag, "Resolved deployed tag to commit SHA");
        Ok(sha)
    }

    /// Dereference an annotated tag object to the underlying commit SHA.
    async fn resolve_tag_to_commit_sha(&self, tag_object_sha: &str) -> Result<String> {
        let route = format!(
            "/repos/{owner}/{repo}/git/tags/{sha}",
            owner = self.owner,
            repo = self.repo,
            sha = tag_object_sha,
        );

        let tag: GitTagWithObject = self
            .octocrab
            .get(route, None::<&()>)
            .await
            .context("Failed to fetch annotated tag object from GitHub")?;

        anyhow::ensure!(
            tag.object.r#type == "commit",
            "Annotated tag points to a '{}', not a commit (SHA {})",
            tag.object.r#type,
            tag.object.sha,
        );

        Ok(tag.object.sha)
    }

    /// Download the dataset tarball for a given ref and extract to a temp
    /// directory. Returns the [`TempDir`](tempfile::TempDir) containing the
    /// extracted contents.
    #[instrument(level = "info", skip(self))]
    async fn download_and_extract(&self, ref_name: &str) -> Result<tempfile::TempDir> {
        tracing::info!("Downloading dataset tarball from GitHub");

        let response = self
            .octocrab
            .repos(&self.owner, &self.repo)
            .download_tarball(ref_name.to_string())
            .await
            .context("Failed to download tarball from GitHub")?;

        // Collect the streaming body into Bytes
        let body = response.into_body();
        let collected = body
            .collect()
            .await
            .map_err(|err| anyhow::anyhow!("Failed to read tarball response body: {err}"))?;
        let bytes: Bytes = collected.to_bytes();

        tracing::info!(size_bytes = bytes.len(), "Downloaded tarball");

        // Create temp dir as a sibling of the coverage directory to guarantee
        // both are on the same filesystem, which is required for atomic renames.
        let temp_parent = self.coverage_dir.parent().unwrap_or(Path::new("."));
        let temp_dir =
            tempfile::TempDir::new_in(temp_parent).context("Failed to create temp directory")?;

        // Extract tarball (tar.gz) in a blocking task
        let temp_path = temp_dir.path().to_path_buf();
        tokio::task::spawn_blocking(move || {
            let decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut archive = tar::Archive::new(decoder);
            archive
                .unpack(&temp_path)
                .context("Failed to extract tarball")
        })
        .await
        .context("Tarball extraction task panicked")??;

        Ok(temp_dir)
    }

    /// Find the `dataset/` subdirectory inside the extracted tarball root.
    ///
    /// GitHub tarballs contain a single top-level directory named
    /// `{owner}-{repo}-{short_sha}/` with the repository contents inside.
    fn find_dataset_dir(extracted: &Path) -> Result<PathBuf> {
        let top_level = std::fs::read_dir(extracted)
            .context("Failed to list extracted tarball contents")?
            .filter_map(|e| e.ok())
            .find(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .context("No top-level directory found in tarball")?;

        let dataset_dir = top_level.path().join("dataset");
        if dataset_dir.is_dir() {
            Ok(dataset_dir)
        } else {
            anyhow::bail!(
                "No dataset/ directory found in tarball (looked in {})",
                top_level.path().display()
            )
        }
    }

    /// Download, validate and install the dataset for a given git ref.
    ///
    /// `commit_sha` is the resolved commit SHA that gets persisted as the
    /// version marker. `ref_name` is the ref passed to the tarball API (can
    /// be the same SHA, a tag name, or a branch name).
    ///
    /// Returns the loaded [`Network`] on success so the caller can swap it in.
    #[instrument(level = "info", skip(self))]
    pub async fn fetch_and_install(&self, ref_name: &str, commit_sha: &str) -> Result<Network> {
        let temp_dir = self.download_and_extract(ref_name).await?;
        let dataset_dir = Self::find_dataset_dir(temp_dir.path())?;

        // Validate by loading - this catches any schema / parse errors before
        // we touch the on-disk copy.
        let dataset_path = dataset_dir.to_string_lossy().to_string();
        tracing::info!(%dataset_path, "Validating downloaded dataset");

        let network = tokio::task::spawn_blocking(move || Network::load_from_dir(&dataset_path))
            .await
            .context("Dataset load task panicked")?
            .map_err(|errs| anyhow::anyhow!("Failed to parse dataset: {errs:?}"))?;

        // Atomically swap the on-disk coverage directory.
        let cov = self.coverage_dir.clone();
        let src = dataset_dir.clone();
        tokio::task::spawn_blocking(move || atomic_replace_dir(&src, &cov))
            .await
            .context("Directory replacement task panicked")??;

        // Persist the commit SHA so subsequent startups know what we have.
        self.save_sha(commit_sha)?;

        tracing::info!(%ref_name, %commit_sha, "Dataset installed successfully");
        Ok(network)
    }

    /// Synchronise the dataset on startup.
    ///
    /// 1. Resolve the `deployed/<env>` tag to a commit SHA.
    /// 2. Compare with the locally stored SHA.
    /// 3. If they differ (or no local SHA exists), download and install.
    ///
    /// Returns `Some(Network)` when a new dataset was installed, or `None` when
    /// the local copy is already up-to-date (or GitHub is unreachable).
    #[instrument(level = "info", skip(self))]
    pub async fn sync_on_startup(&self) -> Result<Option<Network>> {
        let local_sha = self.local_sha();

        let deployed_sha = match self.resolve_deployed_sha().await {
            Ok(sha) => sha,
            Err(err) => {
                tracing::warn!(
                    ?err,
                    tag = %self.deployed_tag,
                    "Failed to resolve deployed tag, using cached dataset"
                );
                return Ok(None);
            }
        };

        tracing::info!(%deployed_sha, local = ?local_sha, "Checking dataset version");

        if local_sha.as_deref() == Some(deployed_sha.as_str()) {
            tracing::info!(%deployed_sha, "Dataset is already up-to-date");
            return Ok(None);
        }

        tracing::info!(
            from = ?local_sha,
            to = %deployed_sha,
            tag = %self.deployed_tag,
            "Dataset update available, downloading"
        );

        match self.fetch_and_install(&deployed_sha, &deployed_sha).await {
            Ok(network) => Ok(Some(network)),
            Err(err) => {
                tracing::warn!(
                    ?err,
                    "Failed to download dataset update, using cached dataset"
                );
                Ok(None)
            }
        }
    }
}

/// Atomically replace `dst` with `src` using directory renames.
///
/// Both paths **must** reside on the same filesystem (otherwise `rename`
/// returns `EXDEV`). The function creates a `.old` backup of `dst` and
/// rolls back if the swap fails.
fn atomic_replace_dir(src: &Path, dst: &Path) -> Result<()> {
    let backup = dst.with_extension("old");

    // Clean up any leftover backup from a previous failed swap.
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .with_context(|| format!("Failed to remove stale backup at {}", backup.display()))?;
    }

    if dst.exists() {
        // Step 1: current → backup
        std::fs::rename(dst, &backup).with_context(|| {
            format!("Failed to rename {} → {}", dst.display(), backup.display())
        })?;

        // Step 2: new → current
        if let Err(err) = std::fs::rename(src, dst) {
            tracing::error!(?err, "Failed to move new dataset into place, rolling back");
            std::fs::rename(&backup, dst).with_context(|| {
                format!(
                    "CRITICAL: rollback rename {} → {} also failed",
                    backup.display(),
                    dst.display()
                )
            })?;
            return Err(err).with_context(|| {
                format!("Failed to rename {} → {}", src.display(), dst.display())
            });
        }
    } else {
        // No existing directory - just move into place.
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent of {}", dst.display()))?;
        }
        std::fs::rename(src, dst)
            .with_context(|| format!("Failed to rename {} → {}", src.display(), dst.display()))?;
    }

    // Step 3: clean up backup (non-fatal)
    if backup.exists()
        && let Err(err) = std::fs::remove_dir_all(&backup)
    {
        tracing::warn!(
            ?err,
            path = %backup.display(),
            "Failed to remove backup directory, will be cleaned up on next deploy"
        );
    }

    Ok(())
}

#[cfg(feature = "test-utils")]
impl DatasetManager {
    /// Create a dummy `DatasetManager` for use in tests.
    ///
    /// The octocrab client is unauthenticated and points at a fake repo -
    /// none of the fetch methods will succeed, but the struct can be stored
    /// in [`AppState`](crate::state::AppState).
    pub fn new_test(coverage_dir: impl Into<PathBuf>) -> Self {
        Self {
            octocrab: Octocrab::builder().build().unwrap(),
            owner: "test-owner".to_string(),
            repo: "test-repo".to_string(),
            deployed_tag: "deployed/test".to_string(),
            coverage_dir: coverage_dir.into(),
        }
    }
}
