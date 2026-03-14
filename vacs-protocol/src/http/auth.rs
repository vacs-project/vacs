use crate::vatsim::ClientId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct InitVatsimLogin {
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct AuthExchangeToken {
    pub code: String,
    pub state: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct UserInfo {
    pub cid: ClientId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AuthTokenResponse {
    pub cid: ClientId,
    pub token: String,
}
