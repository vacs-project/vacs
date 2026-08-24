#![cfg(target_os = "linux")]

use std::time::Duration;
use vacs_protocol::http::webrtc::IceConfig;
use vacs_webrtc::Peer;

fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

async fn gather_and_close() {
    let (mut peer, _events) = Peer::new(
        IceConfig {
            ice_servers: Vec::new(),
            expires_at: None,
        },
        false,
    )
    .await
    .expect("failed to create peer");

    let _ = peer.create_offer().await.expect("failed to create offer");
    tokio::time::sleep(Duration::from_millis(300)).await;

    peer.close().await.expect("failed to close peer");
    drop(peer);
}

async fn pump_candidates(
    mut events: tokio::sync::broadcast::Receiver<vacs_webrtc::PeerEvent>,
    other: std::sync::Arc<tokio::sync::Mutex<Peer>>,
    connected_tx: tokio::sync::mpsc::Sender<()>,
) {
    loop {
        match events.recv().await {
            Ok(vacs_webrtc::PeerEvent::IceCandidate(candidate)) => {
                let other = other.lock().await;
                let _ = other.add_remote_ice_candidate(candidate).await;
            }
            Ok(vacs_webrtc::PeerEvent::ConnectionState(
                vacs_webrtc::PeerConnectionState::Connected,
            )) => {
                let _ = connected_tx.send(()).await;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

async fn connect_and_close_pair() {
    let config = || IceConfig {
        ice_servers: Vec::new(),
        expires_at: None,
    };
    let (offerer, offerer_events) = Peer::new(config(), false).await.expect("offerer");
    let (answerer, answerer_events) = Peer::new(config(), false).await.expect("answerer");

    let offer = offerer.create_offer().await.expect("offer");
    let answer = answerer.accept_offer(offer).await.expect("answer");
    offerer.accept_answer(answer).await.expect("accept answer");

    let offerer = std::sync::Arc::new(tokio::sync::Mutex::new(offerer));
    let answerer = std::sync::Arc::new(tokio::sync::Mutex::new(answerer));

    let (connected_tx, mut connected_rx) = tokio::sync::mpsc::channel(4);
    let pump_a = tokio::spawn(pump_candidates(
        offerer_events,
        answerer.clone(),
        connected_tx.clone(),
    ));
    let pump_b = tokio::spawn(pump_candidates(
        answerer_events,
        offerer.clone(),
        connected_tx,
    ));

    // Generous timeout: under a full parallel workspace test run the loopback
    // connection can take far longer than it does in isolation.
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(60), connected_rx.recv())
            .await
            .expect("peers did not connect in time");
    }

    offerer.lock().await.close().await.expect("close offerer");
    answerer.lock().await.close().await.expect("close answerer");
    pump_a.abort();
    pump_b.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_connected_peer_pair_releases_its_sockets() {
    connect_and_close_pair().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let baseline = open_fds();

    for _ in 0..3 {
        connect_and_close_pair().await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after = open_fds();

    assert!(
        after <= baseline + 2,
        "file descriptors leaked across connected peer lifecycles: {baseline} before, {after} after"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_peer_releases_its_sockets() {
    gather_and_close().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let baseline = open_fds();

    for _ in 0..5 {
        gather_and_close().await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after = open_fds();

    assert!(
        after <= baseline + 2,
        "file descriptors leaked across peer lifecycles: {baseline} before, {after} after"
    );
}
