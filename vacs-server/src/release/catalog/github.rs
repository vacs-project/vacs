use crate::APP_USER_AGENT;
use crate::http::error::AppError;
use crate::release::catalog::{BundleType, Catalog, ReleaseAsset, ReleaseMeta};
use anyhow::Context;
use async_trait::async_trait;
use lru::LruCache;
use octocrab::Octocrab;
use octocrab::models::repos::{Asset as OctocrabAsset, Release as OctocrabRelease};
use parking_lot::RwLock;
use regex::{Regex, regex};
use reqwest::header;
use semver::Version;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};
use tracing::instrument;
use vacs_protocol::http::version::ReleaseChannel;

pub use crate::config::GitHubCredentials;

const GITHUB_RELEASE_BODY_REQUIRED_UPDATE_FLAG: &str = "## Mandatory Update";
const GITHUB_RELEASE_BODY_BREAKING_CHANGES_FLAG: &str = "### ⚠ BREAKING CHANGES";
const MAX_PAGINATION_PAGES: usize = 10; // Max 1000 releases (100 per page)
const SIGNATURE_CACHE_SIZE: usize = 100; // LRU cache size for signatures
const FETCH_SIGNATURES_CONCURRENT_REQUESTS: usize = 5; // Number of asset signature downloads to fetch concurrently

/// Matches release titles such as `vacs-client: v1.2.3`, capturing the version.
///
/// Shared by title validation and version parsing, so it lives behind a helper
/// instead of being repeated at both call sites.
fn title_pattern() -> &'static Regex {
    regex!(r"^vacs(-client)?:?[ -]v(?P<version>\d+\.\d+\.\d+(?:[-+].*)?)$")
}

pub struct GitHubCatalog {
    owner: String,
    repo: String,

    client: Octocrab,
    http_client: reqwest::Client,

    releases: RwLock<HashMap<Version, ReleaseMeta>>,
    releases_updated_at: RwLock<Option<Instant>>,
    release_cache_ttl: Duration,

    stable_versions: RwLock<Vec<Version>>,
    beta_versions: RwLock<Vec<Version>>,
    rc_versions: RwLock<Vec<Version>>,

    signatures: RwLock<LruCache<String, (Instant, String)>>,
    signature_cache_ttl: Duration,
}

impl GitHubCatalog {
    const HTTP_CLIENT_DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    #[instrument(
        level = "info",
        skip(credentials, release_cache_ttl, signature_cache_ttl),
        err
    )]
    pub async fn new(
        owner: String,
        repo: String,
        credentials: Option<GitHubCredentials>,
        release_cache_ttl: Duration,
        signature_cache_ttl: Duration,
    ) -> Result<Self, AppError> {
        Self::with_api_base(
            owner,
            repo,
            credentials,
            release_cache_ttl,
            signature_cache_ttl,
            None,
        )
        .await
    }

    async fn with_api_base(
        owner: String,
        repo: String,
        credentials: Option<GitHubCredentials>,
        release_cache_ttl: Duration,
        signature_cache_ttl: Duration,
        api_base: Option<&str>,
    ) -> Result<Self, AppError> {
        let mut client_builder =
            Octocrab::builder().add_header(header::USER_AGENT, APP_USER_AGENT.to_string());

        if let Some(api_base) = api_base {
            client_builder = client_builder
                .base_uri(api_base)
                .context("Failed to set GitHub API base URI")?;
        }

        let client = if let Some(credentials) = credentials {
            tracing::info!(?credentials, "Using GitHub app authentication");

            client_builder = client_builder.app(
                credentials.app_id.into(),
                jsonwebtoken::EncodingKey::from_rsa_pem(credentials.app_private_key.as_bytes())
                    .context("Failed to parse GitHub app private key")?,
            );

            client_builder
                .build()
                .context("Failed to build Octocrab client")?
                .installation(credentials.installation_id.into())
                .context("Failed to set installation ID")?
        } else {
            client_builder
                .build()
                .context("Failed to build Octocrab client")?
        };

        let http_client = reqwest::Client::builder()
            .user_agent(APP_USER_AGENT)
            .timeout(Self::HTTP_CLIENT_DEFAULT_TIMEOUT)
            .build()
            .context("Failed to build HTTP client")?;

        let catalog = Self {
            owner,
            repo,

            client,
            http_client,

            releases: RwLock::new(HashMap::new()),
            releases_updated_at: RwLock::new(None),
            release_cache_ttl,

            stable_versions: RwLock::new(Vec::new()),
            beta_versions: RwLock::new(Vec::new()),
            rc_versions: RwLock::new(Vec::new()),

            signatures: RwLock::new(LruCache::new(
                NonZeroUsize::new(SIGNATURE_CACHE_SIZE).unwrap(),
            )),
            signature_cache_ttl,
        };

        catalog
            .fetch_releases()
            .await
            .context("Failed to fetch releases")?;

        Ok(catalog)
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn fetch_releases(&self) -> Result<(), AppError> {
        tracing::trace!("Fetching releases");

        let raw_releases = self.fetch_all_releases().await?;
        tracing::trace!(total_releases = ?raw_releases.len(), "Fetched all releases");

        let mut by_version = HashMap::new();
        let mut stable_versions = Vec::new();
        let mut beta_versions = Vec::new();
        let mut rc_versions = Vec::new();

        for release in raw_releases {
            if let Some(meta) = self.filter_map_release(&release) {
                let version = meta.version.clone();

                match meta.channel {
                    ReleaseChannel::Stable => stable_versions.push(version.clone()),
                    ReleaseChannel::Beta => beta_versions.push(version.clone()),
                    ReleaseChannel::Rc => rc_versions.push(version.clone()),
                }

                by_version.insert(version, meta);
            }
        }

        stable_versions.sort();
        beta_versions.sort();
        rc_versions.sort();

        tracing::trace!(
            stable = stable_versions.len(),
            beta = beta_versions.len(),
            rc = rc_versions.len(),
            "Partitioned releases by channel"
        );

        {
            *self.releases.write() = by_version;
            *self.stable_versions.write() = stable_versions;
            *self.beta_versions.write() = beta_versions;
            *self.rc_versions.write() = rc_versions;
            *self.releases_updated_at.write() = Some(Instant::now());
        }

        self.prefetch_latest_signatures().await;

        tracing::trace!(
            total_releases = self.releases.read().len(),
            stable_releases = self.stable_versions.read().len(),
            beta_releases = self.beta_versions.read().len(),
            rc_releases = self.rc_versions.read().len(),
            cached_signatures = self.signatures.read().len(),
            "Successfully fetched releases"
        );
        Ok(())
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn fetch_all_releases(&self) -> Result<Vec<OctocrabRelease>, AppError> {
        let mut all_releases = Vec::new();
        let mut page_count = 0;

        let mut current_page = self
            .client
            .repos(&self.owner, &self.repo)
            .releases()
            .list()
            .per_page(100)
            .send()
            .await
            .context("Failed to fetch releases")?;

        tracing::trace!(
            items = current_page.items.len(),
            "Fetched initial page of releases"
        );
        all_releases.extend(current_page.take_items());

        // Fetch additional pages with limit
        while let Ok(Some(mut new_page)) = self.client.get_page(&current_page.next).await {
            page_count += 1;

            if page_count >= MAX_PAGINATION_PAGES {
                tracing::warn!(
                    max_pages = MAX_PAGINATION_PAGES,
                    "Reached maximum pagination limit, stopping"
                );
                break;
            }

            tracing::trace!(
                items = new_page.items.len(),
                page = page_count + 1,
                "Fetched next page of releases"
            );
            all_releases.extend(new_page.take_items());
            current_page = new_page;
        }

        Ok(all_releases)
    }

    #[instrument(level = "debug", skip(self))]
    async fn prefetch_latest_signatures(&self) {
        let versions_to_prefetch = {
            let stable = self.stable_versions.read();
            let beta = self.beta_versions.read();
            let rc = self.rc_versions.read();

            [
                stable.last().cloned(),
                beta.last().cloned(),
                rc.last().cloned(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
        };

        if versions_to_prefetch.is_empty() {
            tracing::debug!("No releases to prefetch signatures for");
            return;
        }

        let releases_to_prefetch = {
            let releases = self.releases.read();

            versions_to_prefetch
                .into_iter()
                .filter_map(|version| releases.get(&version).cloned())
                .collect::<Vec<_>>()
        };

        let sig_tasks: Vec<_> = releases_to_prefetch
            .iter()
            .flat_map(|release| release.assets.iter().map(move |asset| asset.url.clone()))
            .collect();

        tracing::debug!(
            release_count = releases_to_prefetch.len(),
            signature_count = sig_tasks.len(),
            "Prefetching signatures for latest releases"
        );

        use futures_util::stream::{self, StreamExt};
        stream::iter(sig_tasks)
            .map(|asset_url| async move {
                match self.fetch_signature_for_asset(&asset_url).await {
                    Ok(sig) => {
                        tracing::trace!(%asset_url, "Prefetched signature");
                        Some((asset_url, sig))
                    }
                    Err(err) => {
                        tracing::warn!(%asset_url, ?err, "Failed to prefetch signature");
                        None
                    }
                }
            })
            .buffer_unordered(FETCH_SIGNATURES_CONCURRENT_REQUESTS)
            .for_each(|result| async {
                if let Some((url, sig)) = result {
                    self.signatures.write().put(url, (Instant::now(), sig));
                }
            })
            .await;

        tracing::debug!(
            cached_signatures = self.signatures.read().len(),
            "Finished prefetching signatures for latest releases"
        );
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn fetch_signature_for_asset(&self, asset_url: &str) -> Result<String, AppError> {
        let response = self
            .http_client
            .get(format!("{asset_url}.sig"))
            .send()
            .await
            .context("Failed to download signature file")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            tracing::warn!(
                ?status,
                ?body,
                "Received error while download signature file"
            );
            return Err(anyhow::anyhow!("Failed to download signature file: {status}").into());
        }

        Ok(body)
    }

    #[instrument(level = "trace", skip(self, release), fields(release_name = ?release.name, release_tag_name = ?release.tag_name))]
    fn filter_map_release(&self, release: &OctocrabRelease) -> Option<ReleaseMeta> {
        if release.draft {
            tracing::trace!("Ignoring draft release");
            return None;
        }

        if !title_pattern().is_match(release.name.as_ref()?) {
            tracing::trace!("Ignoring release due to name mismatch");
            return None;
        }

        tracing::trace!("Processing release");

        let version =
            self.parse_release_version(release.name.as_ref()?, release.tag_name.as_str())?;
        tracing::trace!(?version, "Parsed release version");

        let channel = self.derive_release_channel(&version, release.prerelease);
        tracing::trace!(?version, ?channel, "Parsed release channel");

        let assets: Vec<ReleaseAsset> = release
            .assets
            .iter()
            .filter_map(|a| self.filter_map_release_asset(a))
            .collect();

        if assets.is_empty() {
            tracing::trace!(?version, ?channel, "Ignoring release due to missing assets");
            return None;
        }

        tracing::trace!(?version, ?channel, assets = ?assets.len(), "Parsed release assets");

        let meta = ReleaseMeta {
            id: release.id.0,
            version,
            channel,
            required: self.is_release_required(&release.body),
            notes: release.body.clone(),
            pub_date: release.published_at.map(|t| t.to_rfc3339()),
            assets,
        };

        tracing::trace!(?meta, "Parsed release");
        Some(meta)
    }

    #[instrument(level = "trace", skip(self, asset), fields(asset_name = ?asset.name))]
    fn filter_map_release_asset(&self, asset: &OctocrabAsset) -> Option<ReleaseAsset> {
        if regex!(r"^SHA256SUMS|(?i)\.sig$").is_match(asset.name.as_str()) {
            tracing::trace!("Ignoring release asset due to name mismatch");
            return None;
        }

        let bundle_type = BundleType::from_file_name(asset.name.as_str())?;

        Some(ReleaseAsset {
            name: asset.name.clone(),
            bundle_type,
            target: bundle_type.target().to_string(),
            arch: self
                .derive_release_asset_arch(asset.name.as_str())?
                .to_string(),
            url: asset.browser_download_url.to_string(),
            signature: None, // Lazy-loaded on demand
        })
    }

    fn parse_release_version(&self, title: &str, tag: &str) -> Option<Version> {
        let semver = regex!(r"v?(?P<version>\d+\.\d+\.\d+(?:[-+].*)?)");

        Version::parse(
            title_pattern()
                .captures(title)
                .or_else(|| semver.captures(title))
                .or_else(|| semver.captures(tag))?
                .name("version")?
                .as_str(),
        )
        .ok()
    }

    fn derive_release_channel(&self, version: &Version, prerelease: bool) -> ReleaseChannel {
        if prerelease || !version.pre.is_empty() {
            if regex!(r"(?i)^rc").is_match(version.pre.as_str()) {
                ReleaseChannel::Rc
            } else {
                ReleaseChannel::Beta
            }
        } else {
            ReleaseChannel::Stable
        }
    }

    fn is_release_required(&self, body: &Option<String>) -> bool {
        match body {
            Some(body) => {
                body.contains(GITHUB_RELEASE_BODY_REQUIRED_UPDATE_FLAG)
                    || body.contains(GITHUB_RELEASE_BODY_BREAKING_CHANGES_FLAG)
            }
            None => false,
        }
    }

    fn derive_release_asset_arch(&self, asset_name: &str) -> Option<&str> {
        if regex!(r"(?i)(x86_64|amd64|x64)").is_match(asset_name) {
            Some("x86_64")
        } else if regex!(r"(?i)(aarch64|arm64)").is_match(asset_name) {
            Some("aarch64")
        } else if regex!(r"(?i)(armv7)").is_match(asset_name) {
            Some("armv7")
        } else {
            None
        }
    }
}

#[async_trait]
impl Catalog for GitHubCatalog {
    #[instrument(level = "debug", skip(self), err)]
    async fn list(&self, channel: ReleaseChannel) -> Result<Vec<ReleaseMeta>, AppError> {
        let should_update = self
            .releases_updated_at
            .read()
            .as_ref()
            .map_or(true, |t| t.elapsed() > self.release_cache_ttl);

        if should_update {
            tracing::debug!("Cache TTL expired, fetching releases");
            self.fetch_releases()
                .await
                .context("Failed to fetch releases")?;
        }

        let versions = match channel {
            ReleaseChannel::Stable => self.stable_versions.read().clone(),
            ReleaseChannel::Beta => self.beta_versions.read().clone(),
            ReleaseChannel::Rc => self.rc_versions.read().clone(),
        };

        let releases = self.releases.read();
        let result = versions
            .iter()
            .filter_map(|v| releases.get(v).cloned())
            .collect();

        Ok(result)
    }

    #[instrument(level = "debug", skip(self, _meta), fields(release_id = %_meta.id, release_version = ?_meta.version), err)]
    async fn load_signature(
        &self,
        _meta: &ReleaseMeta,
        asset: &ReleaseAsset,
    ) -> Result<String, AppError> {
        {
            let mut signatures = self.signatures.write();
            if let Some((cached_at, sig)) = signatures.get(&asset.url) {
                if cached_at.elapsed() < self.signature_cache_ttl {
                    tracing::trace!("Using cached signature for release asset");
                    return Ok(sig.clone());
                } else {
                    tracing::trace!("Cached signature expired");
                    signatures.pop(&asset.url);
                }
            }
        }

        tracing::debug!("Fetching signature for release asset");
        match self.fetch_signature_for_asset(&asset.url).await {
            Ok(sig) => {
                self.signatures
                    .write()
                    .put(asset.url.clone(), (Instant::now(), sig.clone()));
                Ok(sig)
            }
            Err(err) => {
                tracing::warn!(%asset.url, ?err, "No matching signature found for release asset");
                Err(AppError::NotFound)
            }
        }
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn refresh(&self) -> Result<(), AppError> {
        self.fetch_releases().await
    }
}

impl Debug for GitHubCatalog {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubCatalog")
            .field("owner", &self.owner)
            .field("repo", &self.repo)
            .field("release_cache_ttl", &self.release_cache_ttl)
            .field("signature_cache_ttl", &self.signature_cache_ttl)
            .field("total_releases", &self.releases.read().len())
            .field("stable_releases", &self.stable_versions.read().len())
            .field("beta_releases", &self.beta_versions.read().len())
            .field("rc_releases", &self.rc_versions.read().len())
            .field("cached_signatures", &self.signatures.read().len())
            .finish_non_exhaustive()
    }
}

pub(super) fn default_release_cache_ttl() -> Duration {
    Duration::from_hours(4)
}

pub(super) fn default_signature_cache_ttl() -> Duration {
    Duration::from_hours(24)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use test_log::test;
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const OWNER: &str = "vacs-project";
    const REPO: &str = "vacs";

    fn bundles(version: &str) -> Vec<String> {
        [
            "SHA256SUMS-{v}.txt",
            "SHA256SUMS-{v}.txt.bundle.json",
            "vacs_{v}_amd64.AppImage",
            "vacs_{v}_amd64.AppImage.sig",
            "vacs_{v}_amd64.deb",
            "vacs_{v}_amd64.deb.sig",
            "vacs-{v}-1.x86_64.rpm",
            "vacs-{v}-1.x86_64.rpm.sig",
            "vacs_{v}_x64-setup.exe",
            "vacs_{v}_x64-setup.exe.sig",
            "vacs_{v}_aarch64.app.tar.gz",
            "vacs_{v}_aarch64.app.tar.gz.sig",
            "vacs_{v}_aarch64.dmg",
            "vacs_{v}_x86_64.app.tar.gz",
            "vacs_{v}_x86_64.app.tar.gz.sig",
            "vacs_{v}_x64.dmg",
        ]
        .iter()
        .map(|name| name.replace("{v}", version))
        .collect()
    }

    fn release(id: u64, version: &str, draft: bool, assets: &[String], base: &str) -> Value {
        let tag = format!("vacs-client-v{version}");
        let assets: Vec<Value> = assets
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let asset_id = id * 100 + i as u64;
                json!({
                    "url": format!("{base}/repos/{OWNER}/{REPO}/releases/assets/{asset_id}"),
                    "browser_download_url":
                        format!("{base}/{OWNER}/{REPO}/releases/download/{tag}/{name}"),
                    "id": asset_id,
                    "node_id": format!("RA_{asset_id}"),
                    "name": name,
                    "label": null,
                    "state": "uploaded",
                    "content_type": "application/octet-stream",
                    "size": 1,
                    "download_count": 0,
                    "created_at": "2026-09-21T08:00:00Z",
                    "updated_at": "2026-09-21T08:00:00Z",
                    "uploader": null,
                })
            })
            .collect();
        json!({
            "url": format!("{base}/repos/{OWNER}/{REPO}/releases/{id}"),
            "html_url": format!("{base}/{OWNER}/{REPO}/releases/tag/{tag}"),
            "assets_url": format!("{base}/repos/{OWNER}/{REPO}/releases/{id}/assets"),
            "upload_url": format!("{base}/repos/{OWNER}/{REPO}/releases/{id}/assets{{?name,label}}"),
            "tarball_url": null,
            "zipball_url": null,
            "id": id,
            "node_id": format!("RE_{id}"),
            "tag_name": tag,
            "target_commitish": "main",
            "name": format!("vacs-client: v{version}"),
            "body": "## What's Changed",
            "draft": draft,
            "prerelease": false,
            "created_at": "2026-09-21T08:00:00Z",
            "published_at": if draft { Value::Null } else { json!("2026-09-21T08:30:00Z") },
            "author": null,
            "assets": assets,
        })
    }

    async fn github_api() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"\.sig$"))
            .respond_with(ResponseTemplate::new(200).set_body_string("signature"))
            .mount(&server)
            .await;
        server
    }

    async fn serve_releases(server: &MockServer, releases: Vec<Value>, times: Option<u64>) {
        let mock = Mock::given(method("GET"))
            .and(path(format!("/repos/{OWNER}/{REPO}/releases")))
            .respond_with(ResponseTemplate::new(200).set_body_json(releases));
        match times {
            Some(times) => mock.up_to_n_times(times).mount(server).await,
            None => mock.mount(server).await,
        }
    }

    async fn catalog_for(server: &MockServer) -> GitHubCatalog {
        GitHubCatalog::with_api_base(
            OWNER.to_string(),
            REPO.to_string(),
            None,
            Duration::from_secs(3600),
            Duration::from_secs(3600),
            Some(&server.uri()),
        )
        .await
        .expect("catalog should build against the mock server")
    }

    async fn stable_versions(catalog: &GitHubCatalog) -> Vec<Version> {
        catalog
            .list(ReleaseChannel::Stable)
            .await
            .expect("listing should succeed")
            .into_iter()
            .map(|meta| meta.version)
            .collect()
    }

    #[test(tokio::test)]
    async fn a_draft_release_is_not_listed() {
        let server = github_api().await;
        let base = server.uri();
        serve_releases(
            &server,
            vec![
                release(2, "2.9.0", true, &bundles("2.9.0")[..4], &base),
                release(1, "2.8.0", false, &bundles("2.8.0"), &base),
            ],
            None,
        )
        .await;
        let catalog = catalog_for(&server).await;

        assert_eq!(stable_versions(&catalog).await, vec![Version::new(2, 8, 0)]);
    }

    #[test(tokio::test)]
    async fn a_release_without_bundles_is_not_listed() {
        let server = github_api().await;
        let base = server.uri();
        serve_releases(
            &server,
            vec![release(1, "2.9.0", false, &bundles("2.9.0")[..2], &base)],
            None,
        )
        .await;
        let catalog = catalog_for(&server).await;

        assert!(stable_versions(&catalog).await.is_empty());
    }

    #[test(tokio::test)]
    async fn refresh_picks_up_a_newly_published_release() {
        let server = github_api().await;
        let base = server.uri();
        let old = release(1, "2.8.0", false, &bundles("2.8.0"), &base);
        let new = release(2, "2.9.0", false, &bundles("2.9.0"), &base);
        serve_releases(&server, vec![old.clone()], Some(1)).await;
        serve_releases(&server, vec![old, new], None).await;
        let catalog = catalog_for(&server).await;

        assert_eq!(stable_versions(&catalog).await, vec![Version::new(2, 8, 0)]);

        catalog.refresh().await.expect("refresh should succeed");

        assert_eq!(
            stable_versions(&catalog).await,
            vec![Version::new(2, 8, 0), Version::new(2, 9, 0)]
        );
    }

    #[test]
    fn release_titles_from_the_workflow_are_recognized() {
        for (title, version) in [
            ("vacs-client: v2.9.0", "2.9.0"),
            ("vacs-client-v2.9.0", "2.9.0"),
            ("vacs-client: v2.9.0-rc.1", "2.9.0-rc.1"),
        ] {
            let captures = title_pattern()
                .captures(title)
                .unwrap_or_else(|| panic!("{title} should match"));
            assert_eq!(&captures["version"], version);
        }
        assert!(title_pattern().captures("v2.9.0").is_none());
    }
}
