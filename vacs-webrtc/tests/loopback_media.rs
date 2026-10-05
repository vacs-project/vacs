use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use vacs_audio::EncodedAudioFrame;
use vacs_protocol::http::webrtc::IceConfig;
use vacs_webrtc::{Peer, PeerConnectionState, PeerEvent};

/// In workspace builds, feature unification enables more than one rustls
/// crypto provider, so rustls cannot auto-select one and the DTLS handshake
/// panics unless a process default is installed (the binaries do the same on
/// startup).
fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

async fn pump_candidates(
    mut events: broadcast::Receiver<PeerEvent>,
    other: Arc<Mutex<Peer>>,
    connected_tx: mpsc::Sender<()>,
) {
    loop {
        match events.recv().await {
            Ok(PeerEvent::IceCandidate(candidate)) => {
                let other = other.lock().await;
                let _ = other.add_remote_ice_candidate(candidate).await;
            }
            Ok(PeerEvent::ConnectionState(PeerConnectionState::Connected)) => {
                let _ = connected_tx.send(()).await;
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

struct ConnectedPair {
    offerer: Arc<Mutex<Peer>>,
    answerer: Arc<Mutex<Peer>>,
    pumps: [JoinHandle<()>; 2],
}

impl ConnectedPair {
    async fn new() -> Self {
        install_crypto_provider();

        let config = || IceConfig {
            ice_servers: Vec::new(),
            expires_at: None,
        };
        let (offerer, offerer_events) = Peer::new(config(), false).await.expect("offerer");
        let (answerer, answerer_events) = Peer::new(config(), false).await.expect("answerer");

        let offer = offerer.create_offer().await.expect("offer");
        let answer = answerer.accept_offer(offer).await.expect("answer");
        offerer.accept_answer(answer).await.expect("accept answer");

        let offerer = Arc::new(Mutex::new(offerer));
        let answerer = Arc::new(Mutex::new(answerer));

        let (connected_tx, mut connected_rx) = mpsc::channel(4);
        let pumps = [
            tokio::spawn(pump_candidates(
                offerer_events,
                answerer.clone(),
                connected_tx.clone(),
            )),
            tokio::spawn(pump_candidates(
                answerer_events,
                offerer.clone(),
                connected_tx,
            )),
        ];

        // Generous timeout: under a full parallel workspace test run the loopback
        // connection can take far longer than it does in isolation.
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(60), connected_rx.recv())
                .await
                .expect("peers did not connect in time");
        }

        Self {
            offerer,
            answerer,
            pumps,
        }
    }

    async fn close(self) {
        self.offerer
            .lock()
            .await
            .close()
            .await
            .expect("close offerer");
        self.answerer
            .lock()
            .await
            .close()
            .await
            .expect("close answerer");
        for pump in self.pumps {
            pump.abort();
        }
    }
}

/// Starts `peer` with a 20 ms frame feed, as a call capture stream does even
/// while muted. Abort the returned task to stop feeding.
async fn start_sending(peer: &Mutex<Peer>) -> (JoinHandle<()>, mpsc::Receiver<EncodedAudioFrame>) {
    let (input_tx, input_rx) = broadcast::channel(64);
    let (output_tx, output_rx) = mpsc::channel(256);
    peer.lock()
        .await
        .start(input_rx, output_tx)
        .expect("start peer");

    let feeder = tokio::spawn(async move {
        loop {
            let _ = input_tx.send(EncodedAudioFrame::from_static(&[0xf8, 0xff, 0xfe]));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    (feeder, output_rx)
}

async fn wait_for_no_inbound_media(
    events: &mut broadcast::Receiver<PeerEvent>,
    timeout: Duration,
) -> bool {
    tokio::time::timeout(timeout, async {
        loop {
            match events.recv().await {
                Ok(PeerEvent::NoInboundMedia) => return,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => std::future::pending().await,
            }
        }
    })
    .await
    .is_ok()
}

/// In a conference the newest participant connects several links at once and
/// starts each one only after the app state lock frees up, so its peers may
/// already be sending. The late side must still receive their audio.
#[tokio::test(flavor = "multi_thread")]
async fn peer_started_after_remote_media_arrives_still_receives() {
    let pair = ConnectedPair::new().await;

    let (offerer_feeder, mut offerer_output_rx) = start_sending(&pair.offerer).await;
    // Loopback RTP arrives within milliseconds; the margin keeps the remote's first packet
    // ahead of the late start on a loaded runner, or the test would not reach the race.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (answerer_feeder, mut answerer_output_rx) = start_sending(&pair.answerer).await;

    let (late_received, prompt_received) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(10), answerer_output_rx.recv()),
        tokio::time::timeout(Duration::from_secs(10), offerer_output_rx.recv()),
    );

    offerer_feeder.abort();
    answerer_feeder.abort();
    pair.close().await;

    assert!(
        matches!(late_received, Ok(Some(_))),
        "late started peer received no audio from a remote that was already sending"
    );
    assert!(
        matches!(prompt_received, Ok(Some(_))),
        "peer started before the remote sent received no audio"
    );
}

/// The remote's receiver reports keep the RTCP counter moving, so only the
/// missing RTP shows that its audio never reaches this peer.
#[tokio::test(flavor = "multi_thread")]
async fn watchdog_signals_when_rtcp_flows_but_rtp_never_arrives() {
    let pair = ConnectedPair::new().await;
    let mut offerer_events = pair.offerer.lock().await.subscribe();

    let (offerer_feeder, _offerer_output_rx) = start_sending(&pair.offerer).await;

    let signalled = wait_for_no_inbound_media(&mut offerer_events, Duration::from_secs(30)).await;

    offerer_feeder.abort();
    pair.close().await;

    assert!(signalled, "a peer that never received RTP was not reported");
}

#[tokio::test(flavor = "multi_thread")]
async fn watchdog_stays_quiet_while_both_sides_send() {
    let pair = ConnectedPair::new().await;
    let mut offerer_events = pair.offerer.lock().await.subscribe();
    let mut answerer_events = pair.answerer.lock().await.subscribe();

    let (offerer_feeder, _offerer_output_rx) = start_sending(&pair.offerer).await;
    let (answerer_feeder, _answerer_output_rx) = start_sending(&pair.answerer).await;

    let (offerer_signalled, answerer_signalled) = tokio::join!(
        wait_for_no_inbound_media(&mut offerer_events, Duration::from_secs(8)),
        wait_for_no_inbound_media(&mut answerer_events, Duration::from_secs(8)),
    );

    offerer_feeder.abort();
    answerer_feeder.abort();
    pair.close().await;

    assert!(
        !offerer_signalled && !answerer_signalled,
        "the watchdog reported a healthy link"
    );
}
