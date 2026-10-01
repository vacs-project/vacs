pub mod file;
pub mod github;

use crate::http::error::AppError;
use async_trait::async_trait;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use vacs_protocol::http::version::ReleaseChannel;

#[async_trait]
pub trait Catalog: Send + Sync + 'static {
    async fn list(&self, channel: ReleaseChannel) -> Result<Vec<ReleaseMeta>, AppError>;
    async fn load_signature(
        &self,
        meta: &ReleaseMeta,
        asset: &ReleaseAsset,
    ) -> Result<String, AppError>;

    /// Repopulate the catalog from its backing source, ignoring any cache TTL.
    async fn refresh(&self) -> Result<(), AppError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "backend")]
pub enum CatalogConfig {
    File {
        #[serde(default = "file::default_catalog_path")]
        path: PathBuf,
    },
    GitHub {
        owner: String,
        repo: String,
        #[serde(default)]
        credentials: Option<crate::config::GitHubCredentials>,
        #[serde(default = "github::default_release_cache_ttl")]
        release_cache_ttl: Duration,
        #[serde(default = "github::default_signature_cache_ttl")]
        signature_cache_ttl: Duration,
    },
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self::File {
            path: file::default_catalog_path(),
        }
    }
}

impl CatalogConfig {
    pub async fn to_catalog(&self) -> Result<Arc<dyn Catalog>, AppError> {
        match self {
            CatalogConfig::File { path } => Ok(Arc::new(file::FileCatalog::new(path)?)),
            CatalogConfig::GitHub {
                owner,
                repo,
                credentials,
                release_cache_ttl,
                signature_cache_ttl,
            } => Ok(Arc::new(
                github::GitHubCatalog::new(
                    owner.clone(),
                    repo.clone(),
                    credentials.clone(),
                    *release_cache_ttl,
                    *signature_cache_ttl,
                )
                .await?,
            )),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ReleaseMeta {
    pub id: u64,
    pub version: Version,
    pub channel: ReleaseChannel,
    pub required: bool,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

impl Debug for ReleaseMeta {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReleaseMeta")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("channel", &self.channel)
            .field("required", &self.required)
            .field("has_notes", &self.notes.is_some())
            .field("pub_date", &self.pub_date)
            .field("assets", &self.assets)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum BundleType {
    #[default]
    Unknown,
    AppImage,
    Deb,
    Rpm,
    App,
    Msi,
    Nsis,
}

impl BundleType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            BundleType::Unknown => "unknown",
            BundleType::AppImage => "appimage",
            BundleType::Deb => "deb",
            BundleType::Rpm => "rpm",
            BundleType::App => "app",
            BundleType::Msi => "msi",
            BundleType::Nsis => "nsis",
        }
    }

    pub fn target(&self) -> &str {
        match self {
            BundleType::Unknown => "unknown",
            BundleType::AppImage => "linux",
            BundleType::Deb => "linux",
            BundleType::Rpm => "linux",
            BundleType::App => "darwin",
            BundleType::Msi => "windows",
            BundleType::Nsis => "windows",
        }
    }
}

impl FromStr for BundleType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "unknown" => Ok(BundleType::Unknown),
            "appimage" => Ok(BundleType::AppImage),
            "deb" => Ok(BundleType::Deb),
            "rpm" => Ok(BundleType::Rpm),
            "app" => Ok(BundleType::App),
            "msi" => Ok(BundleType::Msi),
            "nsis" => Ok(BundleType::Nsis),
            _ => Err(format!("unknown bundle type {}", s)),
        }
    }
}

impl TryFrom<&str> for BundleType {
    type Error = String;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for BundleType {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.as_str().parse()
    }
}

impl Display for BundleType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for BundleType {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl BundleType {
    pub fn from_file_name(file_name: &str) -> Option<BundleType> {
        let file_name = file_name.to_ascii_lowercase();
        if file_name.ends_with(".appimage") {
            Some(BundleType::AppImage)
        } else if file_name.ends_with(".deb") {
            Some(BundleType::Deb)
        } else if file_name.ends_with(".rpm") {
            Some(BundleType::Rpm)
        } else if file_name.ends_with(".app.tar.gz") {
            Some(BundleType::App)
        } else if file_name.ends_with(".exe") {
            Some(BundleType::Nsis)
        } else if file_name.ends_with(".msi") {
            Some(BundleType::Msi)
        } else {
            None
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ReleaseAsset {
    #[serde(default)]
    pub name: String,
    pub target: String,
    pub arch: String,
    pub bundle_type: BundleType,
    pub url: String,
    pub signature: Option<String>,
}

impl Debug for ReleaseAsset {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReleaseAsset")
            .field("name", &self.name)
            .field("target", &self.target)
            .field("arch", &self.arch)
            .field("bundle_type", &self.bundle_type)
            .finish_non_exhaustive()
    }
}
