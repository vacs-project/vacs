use crate::http::error::AppError;
use crate::release::catalog::{Catalog, ReleaseAsset, ReleaseMeta};
use anyhow::Context;
use async_trait::async_trait;
use parking_lot::RwLock;
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::instrument;
use vacs_protocol::http::version::ReleaseChannel;

#[derive(Debug)]
pub struct FileCatalog {
    path: PathBuf,
    stable: RwLock<Vec<ReleaseMeta>>,
    beta: RwLock<Vec<ReleaseMeta>>,
    rc: RwLock<Vec<ReleaseMeta>>,
}
impl FileCatalog {
    #[instrument(level = "info", skip_all, err)]
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, AppError> {
        let catalog = Self {
            path: path.into(),
            stable: Default::default(),
            beta: Default::default(),
            rc: Default::default(),
        };
        catalog.reload()?;
        Ok(catalog)
    }

    #[instrument(level = "info", skip(self), err)]
    pub fn reload(&self) -> Result<(), AppError> {
        tracing::debug!(manifest_path = ?self.path, "Reloading FileCatalog");

        if !self.path.is_file() {
            tracing::warn!(manifest_path = ?self.path, "FileCatalog not found, skipping reload");
            return Ok(());
        }

        let bytes =
            fs::read(&self.path).with_context(|| format!("reading manifest {:?}", self.path))?;

        let manifest: ManifestPerChannel = toml::from_slice(&bytes).context("parsing manifest")?;

        let mut stable = assign_channel(ReleaseChannel::Stable, manifest.stable);
        let mut beta = assign_channel(ReleaseChannel::Beta, manifest.beta);
        let mut rc = assign_channel(ReleaseChannel::Rc, manifest.rc);

        validate_and_sort(&mut stable).context("stable channel")?;
        validate_and_sort(&mut beta).context("beta channel")?;
        validate_and_sort(&mut rc).context("rc channel")?;

        {
            *self.stable.write() = stable;
            *self.beta.write() = beta;
            *self.rc.write() = rc;
        }

        tracing::info!(
            manifest_path = ?self.path,
            stable = self.stable.read().len(),
            beta = self.beta.read().len(),
            rc = self.rc.read().len(),
            "FileCatalog reloaded"
        );

        Ok(())
    }
}

#[async_trait]
impl Catalog for FileCatalog {
    #[instrument(level = "debug", skip(self), err)]
    async fn list(&self, channel: ReleaseChannel) -> Result<Vec<ReleaseMeta>, AppError> {
        Ok(match channel {
            ReleaseChannel::Stable => self.stable.read().clone(),
            ReleaseChannel::Beta => self.beta.read().clone(),
            ReleaseChannel::Rc => self.rc.read().clone(),
        })
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn load_signature(
        &self,
        _meta: &ReleaseMeta,
        asset: &ReleaseAsset,
    ) -> Result<String, AppError> {
        // Signatures are always present in the release manifest, so we can just return the value
        // right away without having to load it from the catalog.
        Ok(asset.signature.clone().unwrap_or_default())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestRelease {
    #[serde(default)]
    id: u64,
    version: Version,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    pub_date: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ManifestPerChannel {
    #[serde(default)]
    stable: Vec<ManifestRelease>,
    #[serde(default)]
    beta: Vec<ManifestRelease>,
    #[serde(default, alias = "dev")]
    rc: Vec<ManifestRelease>,
}

fn assign_channel(ch: ReleaseChannel, items: Vec<ManifestRelease>) -> Vec<ReleaseMeta> {
    items
        .into_iter()
        .map(|r| ReleaseMeta {
            id: r.id,
            version: r.version,
            channel: ch,
            required: r.required,
            notes: r.notes,
            pub_date: r.pub_date,
            assets: r.assets,
        })
        .collect()
}

fn validate_and_sort(v: &mut [ReleaseMeta]) -> anyhow::Result<()> {
    v.sort_by(|a, b| a.version.cmp(&b.version));

    for (i, release) in v.iter().enumerate() {
        if i > 0 && v[i - 1].version == release.version {
            anyhow::bail!("duplicate version {}", release.version);
        }
        if release.assets.is_empty() {
            tracing::warn!(?release, "Release has no assets");
        }
        if release.assets.iter().any(|a| a.signature.is_none()) {
            tracing::warn!(
                ?release,
                "Release has missing signature for one or more assets"
            );
        }
    }

    Ok(())
}

pub(super) fn default_catalog_path() -> PathBuf {
    PathBuf::from_str("releases.toml").expect("valid path")
}
