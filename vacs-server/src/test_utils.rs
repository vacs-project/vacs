mod test_client;
mod test_env;
mod ws;

pub use test_client::*;
pub use test_env::*;
pub use ws::*;

use vacs_protocol::vatsim::ClientId;
use vatsim_api::types::CertificateId;

/// Test-only conversions between [`CertificateId`] and [`ClientId`].
pub trait CertificateIdExt {
    fn from_client_id(id: &ClientId) -> CertificateId;
}

impl CertificateIdExt for CertificateId {
    fn from_client_id(id: &ClientId) -> CertificateId {
        CertificateId::new(
            id.as_str()
                .parse()
                .expect("ClientId must be a numeric VATSIM CID"),
        )
    }
}
