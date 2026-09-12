use crate::error::{SignalingError, SignalingRuntimeError};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};
use tracing::instrument;
use vacs_protocol::ws::server::ServerMessage;

/// Represents a waiting request for a message that matches a predicate.
struct MatcherEntry {
    predicate: Box<dyn Fn(&ServerMessage) -> bool + Send + Sync + 'static>,
    responder: oneshot::Sender<ServerMessage>,
}

/// ResponseMatcher holds a queue of waiters that want to match an incoming message.
#[derive(Clone, Default)]
pub struct ResponseMatcher {
    /// Queue of matcher entries waiting for a specific message pattern.
    /// Note: Each matcher is served only once per message fan-out.
    inner: Arc<Mutex<VecDeque<MatcherEntry>>>,
}

impl ResponseMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Waits for an incoming message to match the given predicate with a timeout.
    /// Entries are evaluated in order of appearance and removed from the internal queue in case of a match.
    /// Only the first successful matcher will receive the message.
    ///
    /// # Returns
    ///
    /// - `Ok(Message)` if a matching message was received within the timeout.
    /// - `Err(SignalingError:Timeout)` if the timeout was reached before a matching message was received.
    /// - `Err(SignalingError:Disconnected)` if the Matcher was closed unexpectedly.
    #[instrument(level = "debug", skip(self, predicate), err)]
    pub async fn wait_for_with_timeout<F>(
        &self,
        predicate: F,
        timeout: Duration,
    ) -> Result<ServerMessage, SignalingError>
    where
        F: Fn(&ServerMessage) -> bool + Send + Sync + 'static,
    {
        let (tx, rx) = oneshot::channel();

        let entry = MatcherEntry {
            predicate: Box::new(predicate),
            responder: tx,
        };

        self.inner.lock().await.push_back(entry);

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(_)) => Err(SignalingError::Runtime(
                SignalingRuntimeError::Disconnected(None),
            )),
            Err(_) => Err(SignalingError::Timeout("Matcher timed out".to_string())),
        }
    }

    /// Waits for an incoming message to match the given predicate until one has been received.
    /// Entries are evaluated in order of appearance and removed from the internal queue in case of a match.
    /// Only the first successful matcher will receive the message.
    ///
    /// This internally uses [`wait_for_with_timeout`] and waits for [`std::time::Duration::MAX`],
    /// which results in approximately 584,942,417,355 years of wait time - probably enough for our use cases.
    ///
    /// # Returns
    ///
    /// - `Ok(Message)` if a matching message was received within the timeout.
    /// - `Err(SignalingError:Timeout)` if the timeout was reached before a matching message was received.
    ///     Should you ever make it here, please open an issue with the next generation of project maintainers.
    /// - `Err(SignalingError:Disconnected)` if the Matcher was closed unexpectedly.
    #[instrument(level = "debug", skip(self, predicate), err)]
    pub async fn wait_for<F>(&self, predicate: F) -> Result<ServerMessage, SignalingError>
    where
        F: Fn(&ServerMessage) -> bool + Send + Sync + 'static,
    {
        self.wait_for_with_timeout(predicate, Duration::MAX).await
    }

    /// Called by the receiving task to check if a message completes any match. If so, the message is
    /// forwarded to the matcher awaiting it and not processed any further by [`try_match`].
    #[instrument(level = "debug", skip(self, msg))]
    pub async fn try_match(&self, msg: &ServerMessage) {
        // Awaited, not try_lock: contention with a registering waiter must not
        // silently drop the match and strand that waiter until its timeout.
        let mut queue = self.inner.lock().await;
        if let Some(pos) = queue.iter().position(|entry| (entry.predicate)(msg))
            && let Some(entry) = queue.remove(pos)
        {
            let _ = entry.responder.send(msg.clone());
        }
    }

    /// Clears all currently stored matchers.
    /// This should be called when the transport is disconnected/reset to a clean state to avoid
    /// an inconsistent consumer state.
    pub async fn clear(&self) {
        self.inner.lock().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_matches;
    use test_log::test;
    use vacs_protocol::vatsim::{ClientId, PositionId};
    use vacs_protocol::ws::server;
    use vacs_protocol::ws::server::ClientInfo;

    #[test(tokio::test)]
    async fn wait_for() {
        let matcher = ResponseMatcher::new();

        let matcher_clone = matcher.clone();
        let handle = tokio::spawn(async move {
            matcher_clone
                .wait_for(|msg| matches!(msg, ServerMessage::Disconnected(_)))
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        matcher
            .try_match(&ServerMessage::Disconnected(server::Disconnected {
                reason: server::DisconnectReason::Terminated,
            }))
            .await;

        let result = handle.await.unwrap();
        assert_matches!(result, Ok(ServerMessage::Disconnected(_)));
    }

    #[test(tokio::test)]
    async fn wait_for_content() {
        let matcher = ResponseMatcher::new();
        let msg = ServerMessage::ClientList(server::ClientList {
            clients: vec![ClientInfo {
                id: ClientId::from("client1"),
                position_id: Some(PositionId::from("position1")),
                display_name: "Client 1".to_string(),
                frequency: "100.000".to_string(),
            }],
        });

        let matcher_clone = matcher.clone();
        let handle = tokio::spawn(async move {
            matcher_clone
                .wait_for(|msg| matches!(msg, ServerMessage::ClientList(_)))
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        matcher.try_match(&msg).await;

        let result = handle.await.unwrap();
        assert_matches!(result, Ok(ServerMessage::ClientList(inner)) if inner.clients.len() == 1);
    }

    #[test(tokio::test)]
    async fn wait_for_matching_peer_id() {
        let matcher = ResponseMatcher::new();
        let messages = vec![
            ServerMessage::WebrtcAnswer(vacs_protocol::ws::shared::WebrtcAnswer {
                call_id: vacs_protocol::ws::shared::CallId::new(),
                from_client_id: ClientId::from("client1"),
                to_client_id: ClientId::from("client2"),
                sdp: "sdp1".to_string(),
            }),
            ServerMessage::WebrtcAnswer(vacs_protocol::ws::shared::WebrtcAnswer {
                call_id: vacs_protocol::ws::shared::CallId::new(),
                from_client_id: ClientId::from("client2"),
                to_client_id: ClientId::from("client3"),
                sdp: "sdp2".to_string(),
            }),
            ServerMessage::WebrtcAnswer(vacs_protocol::ws::shared::WebrtcAnswer {
                call_id: vacs_protocol::ws::shared::CallId::new(),
                from_client_id: ClientId::from("client3"),
                to_client_id: ClientId::from("client1"),
                sdp: "sdp3".to_string(),
            }),
        ];

        let matcher_clone = matcher.clone();
        let handle = tokio::spawn(async move {
            matcher_clone
                .wait_for(|msg| matches!(msg, ServerMessage::WebrtcAnswer(ans) if ans.from_client_id == ClientId::from("client2")))
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        for msg in messages {
            matcher.try_match(&msg).await;
        }

        let result = handle.await.unwrap();
        assert_matches!(result, Ok(ServerMessage::WebrtcAnswer(ans)) if ans.from_client_id == ClientId::from("client2") && ans.sdp == "sdp2");
    }

    #[test(tokio::test)]
    async fn wait_for_with_timeout() {
        let matcher = ResponseMatcher::new();
        let msg = ServerMessage::Disconnected(server::Disconnected {
            reason: server::DisconnectReason::Terminated,
        });

        let matcher_clone = matcher.clone();
        let handle = tokio::spawn(async move {
            matcher_clone
                .wait_for_with_timeout(
                    |msg| matches!(msg, ServerMessage::Disconnected(_)),
                    Duration::from_millis(100),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        matcher.try_match(&msg).await;

        let result = handle.await.unwrap();
        assert_matches!(result, Ok(ServerMessage::Disconnected(_)));
    }

    #[test(tokio::test)]
    async fn wait_for_with_timeout_expires() {
        let matcher = ResponseMatcher::new();

        let result = matcher
            .wait_for_with_timeout(
                |msg| matches!(msg, ServerMessage::Disconnected(_)),
                Duration::from_millis(1),
            )
            .await;
        assert_matches!(result, Err(SignalingError::Timeout(_)));
    }

    #[test(tokio::test)]
    async fn try_match_matches_only_once() {
        let matcher = ResponseMatcher::new();

        let m1 = matcher.clone();
        let m2 = matcher.clone();

        let h1 = tokio::spawn(async move {
            m1.wait_for_with_timeout(
                |m| matches!(m, ServerMessage::Disconnected(_)),
                Duration::from_millis(20),
            )
            .await
        });
        let h2 = tokio::spawn(async move {
            m2.wait_for_with_timeout(
                |m| matches!(m, ServerMessage::Disconnected(_)),
                Duration::from_millis(20),
            )
            .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        matcher
            .try_match(&ServerMessage::Disconnected(server::Disconnected {
                reason: server::DisconnectReason::Terminated,
            }))
            .await;

        let r1 = h1.await.unwrap();
        let r2 = h2.await.unwrap();

        // One should succeed, the other one should time out
        assert!(
            matches!(r1, Ok(ServerMessage::Disconnected(_)))
                ^ matches!(r2, Ok(ServerMessage::Disconnected(_)))
        );
    }

    #[test(tokio::test)]
    async fn try_match_with_overlapping_predicates() {
        let matcher = ResponseMatcher::new();

        let m1 = matcher.clone();
        let m2 = matcher.clone();

        let h1 = tokio::spawn(async move {
            m1.wait_for_with_timeout(
                |m| {
                    matches!(
                        m,
                        ServerMessage::Disconnected(_) | ServerMessage::CallError { .. }
                    )
                },
                Duration::from_millis(20),
            )
            .await
        });
        let h2 = tokio::spawn(async move {
            m2.wait_for_with_timeout(
                |m| matches!(m, ServerMessage::Disconnected(_)),
                Duration::from_millis(20),
            )
            .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        matcher
            .try_match(&ServerMessage::Disconnected(server::Disconnected {
                reason: server::DisconnectReason::Terminated,
            }))
            .await;

        let r1 = h1.await.unwrap();
        let r2 = h2.await.unwrap();

        let matches = [r1, r2]
            .iter()
            .filter(|r| matches!(r, Ok(ServerMessage::Disconnected(_))))
            .count();
        assert_eq!(matches, 1);
    }

    #[test(tokio::test)]
    async fn try_match_concurrent_waiters() {
        let matcher = ResponseMatcher::new();

        let barrier = Arc::new(tokio::sync::Barrier::new(11));
        let mut handles = vec![];

        for _ in 0..10 {
            let matcher_clone = matcher.clone();
            let barrier_clone = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier_clone.wait().await;
                matcher_clone
                    .wait_for_with_timeout(
                        |m| matches!(m, ServerMessage::Disconnected(_)),
                        Duration::from_millis(20),
                    )
                    .await
            }));
        }

        barrier.wait().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        matcher
            .try_match(&ServerMessage::Disconnected(server::Disconnected {
                reason: server::DisconnectReason::Terminated,
            }))
            .await;

        let mut matches = 0;
        for h in handles {
            if matches!(h.await.unwrap(), Ok(ServerMessage::Disconnected(_))) {
                matches += 1;
            }
        }
        assert_eq!(matches, 1);
    }

    #[test(tokio::test)]
    async fn try_match_burst() {
        let matcher = ResponseMatcher::new();

        let h1 = matcher.clone();
        let h2 = matcher.clone();

        let w1 = tokio::spawn(async move {
            h1.wait_for(|msg| matches!(msg, ServerMessage::WebrtcAnswer(_)))
                .await
        });

        let w2 = tokio::spawn(async move {
            h2.wait_for(|msg| matches!(msg, ServerMessage::ClientList(_)))
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;

        for _ in 0..10 {
            matcher
                .try_match(&ServerMessage::Disconnected(server::Disconnected {
                    reason: server::DisconnectReason::Terminated,
                }))
                .await;
        }

        matcher
            .try_match(&ServerMessage::ClientList(server::ClientList {
                clients: vec![ClientInfo {
                    id: ClientId::from("client1"),
                    position_id: Some(PositionId::from("position1")),
                    display_name: "Client 1".into(),
                    frequency: "100.000".into(),
                }],
            }))
            .await;
        matcher
            .try_match(&ServerMessage::WebrtcAnswer(
                vacs_protocol::ws::shared::WebrtcAnswer {
                    call_id: vacs_protocol::ws::shared::CallId::new(),
                    from_client_id: ClientId::from("client2"),
                    to_client_id: ClientId::from("client1"),
                    sdp: "sdp2".into(),
                },
            ))
            .await;

        let r1 = w1.await.unwrap();
        let r2 = w2.await.unwrap();

        assert_matches!(r1, Ok(ServerMessage::WebrtcAnswer(_)));
        assert_matches!(r2, Ok(ServerMessage::ClientList(_)));
    }

    #[test(tokio::test)]
    async fn try_match_without_matchers() {
        let matcher = ResponseMatcher::new();
        matcher
            .try_match(&ServerMessage::Disconnected(server::Disconnected {
                reason: server::DisconnectReason::Terminated,
            }))
            .await;
    }
}
