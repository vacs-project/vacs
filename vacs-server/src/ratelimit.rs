use crate::metrics::ErrorMetrics;
use axum_client_ip::ClientIp;
use governor::clock::{Clock, QuantaClock};
use governor::middleware::NoOpMiddleware;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::net::IpAddr;
use std::num::{NonZero, NonZeroU32};
use std::ops::Deref;
use std::time::Duration;
use vacs_protocol::vatsim::ClientId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub enabled: bool,
    pub per_seconds: u64,
    pub burst: NonZeroU32,
}

impl Default for Policy {
    fn default() -> Self {
        Self::new(10, nonzero!(3u32))
    }
}

impl Policy {
    pub fn new(per_seconds: u64, burst: NonZeroU32) -> Self {
        Self {
            enabled: true,
            per_seconds,
            burst,
        }
    }

    pub fn disabled(self) -> Self {
        Self {
            enabled: false,
            ..self
        }
    }

    pub fn quota(&self) -> Quota {
        Quota::with_period(Duration::from_secs(self.per_seconds.max(1)))
            .expect("invalid policy period")
            .allow_burst(self.burst)
    }
}

type KeyedLimiter<K> = RateLimiter<K, DefaultKeyedStateStore<K>, QuantaClock, NoOpMiddleware>;
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Key(pub String);

impl From<String> for Key {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<ClientId> for Key {
    fn from(value: ClientId) -> Self {
        Self(value.to_string())
    }
}

impl From<&ClientId> for Key {
    fn from(value: &ClientId) -> Self {
        Self(value.to_string())
    }
}

impl From<ClientIp> for Key {
    fn from(value: ClientIp) -> Self {
        Self(value.0.to_string())
    }
}

impl From<IpAddr> for Key {
    fn from(value: IpAddr) -> Self {
        Self(value.to_string())
    }
}

impl Borrow<str> for Key {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Deref for Key {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Default)]
pub struct RateLimiters {
    call_invite: Option<KeyedLimiter<Key>>,
    call_invite_per_minute: Option<KeyedLimiter<Key>>,
    failed_auth: Option<KeyedLimiter<Key>>,
    failed_auth_per_minute: Option<KeyedLimiter<Key>>,
    version_update: Option<KeyedLimiter<Key>>,
    version_update_per_minute: Option<KeyedLimiter<Key>>,
    vatsim_token: Option<KeyedLimiter<Key>>,
    vatsim_token_per_minute: Option<KeyedLimiter<Key>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallInviteRejection {
    /// The limit is exhausted; retry after the given duration.
    RateLimited(Duration),
    /// The invite carries more targets than the burst capacity, so it can never pass.
    TooManyTargets,
}

impl RateLimiters {
    /// Checks the call invite limits, charging one token per target. The burst capacity
    /// of the `call_invite` limiter doubles as the maximum target count per invite.
    #[inline]
    pub fn check_call_invite(
        &self,
        key: impl Into<Key>,
        targets: usize,
    ) -> Result<(), CallInviteRejection> {
        let key = key.into();
        let n = NonZeroU32::new(u32::try_from(targets).unwrap_or(u32::MAX).max(1))
            .expect("clamped to at least one");
        Self::check_n(&self.call_invite, "call_invite", &key, n).and_then(|_| {
            Self::check_n(
                &self.call_invite_per_minute,
                "call_invite_per_minute",
                &key,
                n,
            )
        })
    }

    #[inline]
    pub fn check_failed_auth(&self, key: impl Into<Key>) -> Result<(), Duration> {
        let key = key.into();
        Self::check(&self.failed_auth_per_minute, "failed_auth_per_minute", &key)
            .and_then(|_| Self::check(&self.failed_auth, "failed_auth", &key))
    }

    #[inline]
    pub fn check_version_update(&self, key: impl Into<Key>) -> Result<(), Duration> {
        let key = key.into();
        Self::check(
            &self.version_update_per_minute,
            "version_update_per_minute",
            &key,
        )
        .and_then(|_| Self::check(&self.version_update, "version_update", &key))
    }

    #[inline]
    pub fn check_vatsim_token(&self, key: impl Into<Key>) -> Result<(), Duration> {
        let key = key.into();
        Self::check(
            &self.vatsim_token_per_minute,
            "vatsim_token_per_minute",
            &key,
        )
        .and_then(|_| Self::check(&self.vatsim_token, "vatsim_token", &key))
    }

    #[inline]
    fn check(
        limiter: &Option<KeyedLimiter<Key>>,
        limit_name: impl Into<String>,
        key: &Key,
    ) -> Result<(), Duration> {
        if let Some(limiter) = limiter {
            limiter.check_key(key).map_err(|not_until| {
                ErrorMetrics::rate_limit_exceeded(limit_name);
                not_until.wait_time_from(limiter.clock().now())
            })
        } else {
            Ok(())
        }
    }

    #[inline]
    fn check_n(
        limiter: &Option<KeyedLimiter<Key>>,
        limit_name: impl Into<String>,
        key: &Key,
        n: NonZeroU32,
    ) -> Result<(), CallInviteRejection> {
        let Some(limiter) = limiter else {
            return Ok(());
        };
        match limiter.check_key_n(key, n) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(not_until)) => {
                ErrorMetrics::rate_limit_exceeded(limit_name);
                Err(CallInviteRejection::RateLimited(
                    not_until.wait_time_from(limiter.clock().now()),
                ))
            }
            Err(_) => Err(CallInviteRejection::TooManyTargets),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitersConfig {
    pub enabled: bool,
    pub call_invite: Policy,
    pub call_invite_per_minute: u32,
    pub failed_auth: Policy,
    pub failed_auth_per_minute: u32,
    pub version_update: Policy,
    pub version_update_per_minute: u32,
    pub vatsim_token: Policy,
    pub vatsim_token_per_minute: u32,
}

impl Default for RateLimitersConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            call_invite: Policy::new(10, nonzero!(8u32)),
            call_invite_per_minute: 40,
            failed_auth: Policy::new(60, nonzero!(5u32)).disabled(),
            failed_auth_per_minute: 0, // 60
            version_update: Policy::new(1, nonzero!(10u32)),
            version_update_per_minute: 60,
            vatsim_token: Policy::new(30, nonzero!(3u32)),
            vatsim_token_per_minute: 10,
        }
    }
}

impl From<RateLimitersConfig> for RateLimiters {
    fn from(value: RateLimitersConfig) -> Self {
        if !value.enabled {
            return Self {
                call_invite: None,
                call_invite_per_minute: None,
                failed_auth: None,
                failed_auth_per_minute: None,
                version_update: None,
                version_update_per_minute: None,
                vatsim_token: None,
                vatsim_token_per_minute: None,
            };
        }

        let call_invite = if value.call_invite.enabled {
            Some(KeyedLimiter::<Key>::keyed(value.call_invite.quota()))
        } else {
            None
        };
        let call_invite_per_minute = if value.call_invite_per_minute > 0 {
            let val =
                NonZero::new(value.call_invite_per_minute).expect("invalid call_invite_per_minute");
            Some(KeyedLimiter::<Key>::keyed(
                Quota::per_minute(val).allow_burst(val),
            ))
        } else {
            None
        };

        let failed_auth = if value.failed_auth.enabled {
            Some(KeyedLimiter::<Key>::keyed(value.failed_auth.quota()))
        } else {
            None
        };
        let failed_auth_per_minute = if value.failed_auth_per_minute > 0 {
            let val =
                NonZero::new(value.failed_auth_per_minute).expect("invalid failed_auth_per_minute");
            Some(KeyedLimiter::<Key>::keyed(
                Quota::per_minute(val).allow_burst(val),
            ))
        } else {
            None
        };

        let version_update = if value.version_update.enabled {
            Some(KeyedLimiter::<Key>::keyed(value.version_update.quota()))
        } else {
            None
        };
        let version_update_per_minute = if value.version_update_per_minute > 0 {
            let val = NonZero::new(value.version_update_per_minute)
                .expect("invalid version_update_per_minute");
            Some(KeyedLimiter::<Key>::keyed(
                Quota::per_minute(val).allow_burst(val),
            ))
        } else {
            None
        };

        let vatsim_token = if value.vatsim_token.enabled {
            Some(KeyedLimiter::<Key>::keyed(value.vatsim_token.quota()))
        } else {
            None
        };
        let vatsim_token_per_minute = if value.vatsim_token_per_minute > 0 {
            let val = NonZero::new(value.vatsim_token_per_minute)
                .expect("invalid vatsim_token_per_minute");
            Some(KeyedLimiter::<Key>::keyed(
                Quota::per_minute(val).allow_burst(val),
            ))
        } else {
            None
        };

        Self {
            call_invite,
            call_invite_per_minute,
            failed_auth,
            failed_auth_per_minute,
            version_update,
            version_update_per_minute,
            vatsim_token,
            vatsim_token_per_minute,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_matches;

    fn limiters(burst: u32, per_minute: u32) -> RateLimiters {
        RateLimiters::from(RateLimitersConfig {
            call_invite: Policy::new(600, NonZeroU32::new(burst).unwrap()),
            call_invite_per_minute: per_minute,
            ..RateLimitersConfig::default()
        })
    }

    #[test]
    fn single_target_invites_pass_until_the_burst_is_exhausted() {
        let limiters = limiters(3, 0);

        for _ in 0..3 {
            assert_matches!(limiters.check_call_invite("client1", 1), Ok(()));
        }
        assert_matches!(
            limiters.check_call_invite("client1", 1),
            Err(CallInviteRejection::RateLimited(_))
        );
    }

    #[test]
    fn multi_target_invite_charges_one_token_per_target() {
        let limiters = limiters(3, 0);

        assert_matches!(limiters.check_call_invite("client1", 3), Ok(()));
        assert_matches!(
            limiters.check_call_invite("client1", 1),
            Err(CallInviteRejection::RateLimited(_))
        );
    }

    #[test]
    fn more_targets_than_the_burst_capacity_are_rejected_without_charging() {
        let limiters = limiters(3, 0);

        assert_matches!(
            limiters.check_call_invite("client1", 4),
            Err(CallInviteRejection::TooManyTargets)
        );
        assert_matches!(
            limiters.check_call_invite("client1", 3),
            Ok(()),
            "the rejected oversized invite must not have consumed tokens"
        );
    }

    #[test]
    fn per_minute_limiter_also_charges_per_target() {
        let limiters = limiters(8, 3);

        assert_matches!(limiters.check_call_invite("client1", 3), Ok(()));
        assert_matches!(
            limiters.check_call_invite("client1", 1),
            Err(CallInviteRejection::RateLimited(_))
        );
    }

    #[test]
    fn disabled_limiters_allow_everything() {
        let limiters = RateLimiters::from(RateLimitersConfig {
            enabled: false,
            ..RateLimitersConfig::default()
        });

        assert_matches!(limiters.check_call_invite("client1", 1000), Ok(()));
    }

    #[test]
    fn limits_are_per_client() {
        let limiters = limiters(3, 0);

        assert_matches!(limiters.check_call_invite("client1", 3), Ok(()));
        assert_matches!(limiters.check_call_invite("client2", 3), Ok(()));
    }
}
