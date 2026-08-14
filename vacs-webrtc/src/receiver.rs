use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio::sync::watch;
use tracing::instrument;
use vacs_audio::EncodedAudioFrame;
use webrtc::peer_connection::RTCPeerConnection;

pub struct Receiver {
    shutdown_tx: watch::Sender<()>,
    output_selection_tx: watch::Sender<Option<mpsc::Sender<EncodedAudioFrame>>>,
}

impl Receiver {
    #[instrument(level = "trace", skip_all)]
    pub fn new(
        peer_connection: &RTCPeerConnection,
        output_tx: mpsc::Sender<EncodedAudioFrame>,
        received_packets: Arc<AtomicU64>,
        forwarded_packets: Arc<AtomicU64>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let (output_selection_tx, output_selection_rx) = watch::channel(Some(output_tx));

        peer_connection.on_track(Box::new(move |track, _, _| {
            let mut shutdown_rx = shutdown_rx.clone();
            let mut output_selection_rx = output_selection_rx.clone();
            let received_packets = Arc::clone(&received_packets);
            let forwarded_packets = Arc::clone(&forwarded_packets);

            Box::pin(async move {
                let mut output_tx = output_selection_rx.borrow().clone();

                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown_rx.changed() => {
                            tracing::trace!("Shutdown signalled, stopping receiver");
                            break;
                        }
                        _ = output_selection_rx.changed() => {
                            output_tx = output_selection_rx.borrow().clone();
                        }
                        rtp = track.read_rtp() => {
                            match rtp {
                                Ok((packet, _)) => {
                                    received_packets.fetch_add(1, Ordering::Relaxed);
                                    match output_tx.as_ref() {
                                        Some(output_tx) => {
                                            if output_tx.send(packet.payload).await.is_err() {
                                                tracing::warn!("Failed to send received RTP packet to output");
                                                break;
                                            }
                                            forwarded_packets.fetch_add(1, Ordering::Relaxed);
                                        }
                                        None => {
                                            tracing::trace!("Receiver paused, dropping inbound frame");
                                        }
                                    }
                                }
                                Err(err) => {
                                    tracing::warn!(?err, "Failed to read RTP packet");
                                    break;
                                }
                            }
                        }
                    }
                }
            })
        }));

        Self {
            shutdown_tx,
            output_selection_tx,
        }
    }

    pub fn pause(&self) {
        let _ = self.output_selection_tx.send(None);
    }

    pub fn resume(&self, output_tx: mpsc::Sender<EncodedAudioFrame>) {
        let _ = self.output_selection_tx.send(Some(output_tx));
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.shutdown();
    }
}
