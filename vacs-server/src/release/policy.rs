use crate::http::error::AppError;
use anyhow::Context;
use parking_lot::RwLock;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::instrument;
use vacs_protocol::VACS_PROTOCOL_VERSION;
use vacs_protocol::http::version::ReleaseChannel;

#[derive(Debug)]
pub struct Policy {
    path: PathBuf,
    required_ranges: RwLock<HashMap<ReleaseChannel, Vec<VersionReq>>>,
    compatible_protocol_range: RwLock<VersionReq>,
    visibility: RwLock<HashMap<ReleaseChannel, Vec<ReleaseChannel>>>,
}

impl Policy {
    #[instrument(level = "info", skip_all, err)]
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, AppError> {
        let policy = Self {
            path: path.into(),
            required_ranges: Default::default(),
            compatible_protocol_range: Default::default(),
            visibility: Default::default(),
        };
        policy.reload()?;
        Ok(policy)
    }

    #[instrument(level = "info", skip(self), err)]
    pub fn reload(&self) -> Result<(), AppError> {
        tracing::debug!(policy_path = ?self.path, "Reloading Policy");

        if !self.path.is_file() {
            tracing::warn!(policy_path = ?self.path, "Policy not found, skipping reload");
            return Ok(());
        }

        let bytes =
            fs::read(&self.path).with_context(|| format!("reading policy {:?}", self.path))?;
        let raw_policy: RawPolicy = toml::from_slice(&bytes).context("parsing policy")?;

        let mut required_ranges = HashMap::new();
        for (k, reqs) in raw_policy.required_ranges {
            let parsed = reqs
                .into_iter()
                .map(|s| {
                    VersionReq::parse(&s).with_context(|| {
                        format!("invalid version req '{s}' in required_ranges: {k}")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            required_ranges.insert(
                k.parse()
                    .map_err(|e: String| anyhow::anyhow!(e))
                    .with_context(|| format!("invalid channel key in required_ranges: {k}"))?,
                parsed,
            );
        }

        let compatible_protocol_range = VersionReq::parse(&raw_policy.compatible_protocol_range)
            .with_context(|| {
                format!(
                    "invalid version req '{}' in compatible_protocol_range",
                    raw_policy.compatible_protocol_range
                )
            })?;

        let server_protocol_version = Version::parse(VACS_PROTOCOL_VERSION)
            .context("Failed to parse server protocol crate version")?;
        if !compatible_protocol_range.matches(&server_protocol_version) {
            return Err(anyhow::anyhow!(
                "Protocol version {:?} implemented by server is incompatible with compatible_protocol_range {:?}",
                server_protocol_version,
                compatible_protocol_range
            )
            .into());
        }

        if compatible_protocol_range
            .comparators
            .iter()
            .any(|comp| comp.op == semver::Op::Less || comp.op == semver::Op::LessEq)
        {
            return Err(anyhow::anyhow!(
                "Compatible protocol range must not include a less or less equal comparator"
            )
            .into());
        }

        let mut visibility = HashMap::new();
        for (k, list) in raw_policy.visibility {
            let parsed = list
                .into_iter()
                .map(|s| {
                    s.parse()
                        .map_err(|e: String| anyhow::anyhow!(e))
                        .with_context(|| format!("invalid channel '{s}' in visibility: {k}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            visibility.insert(
                k.parse()
                    .map_err(|e: String| anyhow::anyhow!(e))
                    .with_context(|| format!("invalid channel key in visibility: {k}"))?,
                parsed,
            );
        }

        if visibility.is_empty() {
            visibility.insert(ReleaseChannel::Stable, vec![ReleaseChannel::Stable]);
            visibility.insert(
                ReleaseChannel::Beta,
                vec![ReleaseChannel::Beta, ReleaseChannel::Stable],
            );
            visibility.insert(ReleaseChannel::Rc, vec![ReleaseChannel::Rc]);
        }

        *self.required_ranges.write() = required_ranges;
        *self.compatible_protocol_range.write() = compatible_protocol_range;
        *self.visibility.write() = visibility;

        tracing::info!("Policy reloaded");
        Ok(())
    }

    pub fn is_required(&self, channel: &ReleaseChannel, version: &Version) -> bool {
        let visible = self.visible_channels(channel);

        visible.iter().any(|ch| {
            self.required_ranges
                .read()
                .get(ch)
                .is_some_and(|reqs| reqs.iter().any(|req| req.matches(version)))
        })
    }

    pub fn is_compatible_protocol(&self, version: &Version) -> bool {
        self.compatible_protocol_range.read().matches(version)
    }

    pub fn visible_channels(&self, channel: &ReleaseChannel) -> Vec<ReleaseChannel> {
        let visibility = self.visibility.read();
        visibility
            .get(channel)
            .cloned()
            .unwrap_or_else(|| vec![*channel])
    }
}

#[derive(Deserialize)]
struct RawPolicy {
    #[serde(default)]
    required_ranges: HashMap<String, Vec<String>>,
    #[serde(default = "default_compatible_protocol_range")]
    compatible_protocol_range: String,
    #[serde(default)]
    visibility: HashMap<String, Vec<String>>,
}

fn default_compatible_protocol_range() -> String {
    ">=0.0.0".to_string()
}
