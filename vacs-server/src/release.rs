pub mod catalog;
pub mod policy;

use crate::http::error::AppError;
use crate::release::catalog::file::FileCatalog;
use crate::release::catalog::{BundleType, Catalog, ReleaseAsset, ReleaseMeta};
use crate::release::policy::Policy;
use semver::Version;
use std::sync::Arc;
use tracing::instrument;
use vacs_protocol::http::version::{Release, ReleaseChannel};

pub struct UpdateChecker {
    catalog: Arc<dyn Catalog>,
    policy: Policy,
}

impl UpdateChecker {
    pub fn new(catalog: Arc<dyn Catalog>, policy: Policy) -> Self {
        Self { catalog, policy }
    }

    #[instrument(level = "debug", skip(self), err)]
    pub async fn check(
        &self,
        channel: &ReleaseChannel,
        client_version: &Version,
        target: &String,
        arch: &String,
        bundle_type: &BundleType,
    ) -> Result<Option<Release>, AppError> {
        tracing::debug!("Checking for update");

        let visible = self.policy.visible_channels(channel);

        let mut newer: Vec<(ReleaseMeta, ReleaseAsset)> = Vec::new();
        for ch in &visible {
            for m in self.catalog.list(*ch).await? {
                if m.version > *client_version
                    && let Some(a) = m.assets.iter().find(|a| {
                        a.bundle_type == *bundle_type && a.target == *target && a.arch == *arch
                    })
                {
                    newer.push((m.clone(), a.clone()));
                }
            }
        }

        newer.sort_by(|(a, _), (b, _)| a.version.cmp(&b.version));
        let (meta, asset) = match newer.pop() {
            Some(pair) => pair,
            None => {
                tracing::debug!(?visible, "No update found");
                return Ok(None);
            }
        };

        let required = {
            let mut req = false;
            'outer: for ch in &visible {
                for m in self.catalog.list(*ch).await? {
                    if m.version > *client_version
                        && m.assets.iter().any(|a| {
                            a.bundle_type == *bundle_type && a.target == *target && a.arch == *arch
                        })
                        && (m.required || self.policy.is_required(ch, &m.version))
                    {
                        req = true;
                        break 'outer;
                    }
                }
            }
            req
        };

        let release = Release {
            version: meta.version.to_string(),
            notes: meta.notes.clone(),
            pub_date: meta.pub_date.clone(),
            url: asset.url.clone(),
            signature: self.catalog.load_signature(&meta, &asset).await?,
            required,
        };

        tracing::debug!(?visible, ?release, "Update found");
        Ok(Some(release))
    }

    /// Repopulate the release catalog, ignoring its cache TTL.
    #[instrument(level = "info", skip(self), err)]
    pub async fn refresh_catalog(&self) -> Result<(), AppError> {
        self.catalog.refresh().await
    }

    #[instrument(level = "debug", skip(self))]
    pub fn is_compatible_protocol(&self, protocol_version: Version) -> bool {
        tracing::debug!("Checking client protocol version for compatibility");

        let compatible = self.policy.is_compatible_protocol(&protocol_version);

        tracing::debug!(
            ?compatible,
            "Checked client protocol version for compatibility"
        );
        compatible
    }
}

impl Default for UpdateChecker {
    fn default() -> Self {
        Self::new(
            Arc::new(FileCatalog::new("releases.toml").unwrap()),
            Policy::new("policy.toml").unwrap(),
        )
    }
}
