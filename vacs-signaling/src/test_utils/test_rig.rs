use crate::auth::mock::MockTokenProvider;
use crate::client::{SignalingClient, SignalingEvent};
use crate::test_utils::RecvWithTimeoutExt;
use crate::transport::tokio::TokioTransport;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use vacs_server::test_utils::TestApp;

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
    server: TestApp,
    clients: Vec<TestRigClient>,
    shutdown_token: CancellationToken,
}

impl TestRig {
    pub async fn new(num_clients: usize) -> Self {
        let server = TestApp::new().await;
        let shutdown_token = CancellationToken::new();

        let mut clients = Vec::with_capacity(num_clients);
        for i in 0..num_clients {
            let transport = TokioTransport::new(server.addr());
            let token_provider = MockTokenProvider::new(i, None);

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
            server,
            clients,
            shutdown_token,
        }
    }

    pub fn server(&self) -> &TestApp {
        &self.server
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
