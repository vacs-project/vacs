#[cfg(any(feature = "http", feature = "http-webrtc"))]
pub mod http;
#[cfg(feature = "profile")]
pub mod profile;
#[cfg(feature = "vatsim")]
pub mod vatsim;
#[cfg(feature = "ws")]
pub mod ws;

pub const VACS_PROTOCOL_VERSION: &str = "3.0.0";

#[cfg(feature = "profile")]
pub(crate) mod sealed {
    pub trait Sealed {}
}
