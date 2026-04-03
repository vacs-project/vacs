use crate::auth::mock::MockTokenProvider;
use crate::client::{SignalingClient, SignalingEvent};
use crate::test_utils::RecvWithTimeoutExt;
use crate::transport::tokio::TokioTransport;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use vacs_server::test_utils::TestEnv;

/// Base CID for default test users. User N gets CID `DEFAULT_CID_BASE + N`
/// (i.e. 1000001, 1000002, ...). Must match the value used by
/// [`TestEnv::default_users`].
const DEFAULT_CID_BASE: u32 = 1_000_000;

pub struct TestRigClient {
    pub client: SignalingClient<TokioTransport, MockTokenProvider>,
    pub broadcast_rx: broadcast::Receiver<SignalingEvent>,
}

impl TestRigClient {
    pub async fn recv_with_timeout(&mut self, timeout: Duration) -> Option<SignalingEvent> {
        match tokio::time::timeout(timeout, self.broadcast_rx.recv()).await {
            Ok(Ok(msg)) => Some(msg),
            _ => None,
        }
    }

    pub async fn recv_with_timeout_and_filter<F>(
        &mut self,
        timeout: Duration,
        predicate: F,
    ) -> Option<SignalingEvent>
    where
        F: Fn(&SignalingEvent) -> bool + Send,
    {
        self.broadcast_rx
            .recv_with_timeout(timeout, predicate)
            .await
            .ok()
    }
}

pub struct TestRig {
    env: TestEnv,
    clients: Vec<TestRigClient>,
    shutdown_token: CancellationToken,
}

impl TestRig {
    pub async fn new(num_clients: usize) -> Self {
        let env = TestEnv::builder()
            .default_users(num_clients + 5)
            .build()
            .await;
        let shutdown_token = CancellationToken::new();

        let mut clients = Vec::with_capacity(num_clients);
        for i in 0..num_clients {
            let cid = format!("{}", DEFAULT_CID_BASE + 1 + i as u32);
            let token = env
                .ws_token_for(cid.as_str())
                .await
                .expect("Failed to get WS token");
            let transport = TokioTransport::new(env.ws_url());
            let token_provider = MockTokenProvider::with_token(token, None);

            let client = SignalingClient::new(
                transport,
                token_provider,
                |_| async {},
                shutdown_token.child_token(),
                false,
                Duration::from_millis(100),
                8,
                None,
                &tokio::runtime::Handle::current(),
            );

            let broadcast_rx = client.subscribe();
            client
                .connect(None)
                .await
                .expect("Client failed to connect and login");

            clients.push(TestRigClient {
                client,
                broadcast_rx,
            });
        }

        Self {
            env,
            clients,
            shutdown_token,
        }
    }

    pub fn env(&self) -> &TestEnv {
        &self.env
    }

    pub fn client(&self, index: usize) -> &TestRigClient {
        assert!(
            index < self.clients.len(),
            "Client index {index} out of bounds",
        );
        &self.clients[index]
    }

    pub fn client_mut(&mut self, index: usize) -> &mut TestRigClient {
        assert!(
            index < self.clients.len(),
            "Client index {index} out of bounds",
        );
        &mut self.clients[index]
    }

    pub fn clients(&self) -> &[TestRigClient] {
        &self.clients
    }

    pub fn clients_mut(&mut self) -> &mut [TestRigClient] {
        &mut self.clients
    }

    pub fn shutdown(&self) {
        self.shutdown_token.cancel();
    }
}

impl Drop for TestRig {
    fn drop(&mut self) {
        self.shutdown();
    }
}
