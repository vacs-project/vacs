use crate::auth::TokenProvider;
use crate::error::SignalingError;
use async_trait::async_trait;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct MockTokenProvider {
    token: String,
    delay: Option<Duration>,
}

impl MockTokenProvider {
    pub fn new(client_id: usize, delay: Option<Duration>) -> Self {
        let token = if client_id == usize::MAX {
            String::new()
        } else {
            format!("token{client_id}")
        };
        Self { token, delay }
    }

    pub fn with_token(token: String, delay: Option<Duration>) -> Self {
        Self { token, delay }
    }
}

#[async_trait]
impl TokenProvider for MockTokenProvider {
    async fn get_token(&self) -> Result<String, SignalingError> {
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        Ok(self.token.clone())
    }
}
