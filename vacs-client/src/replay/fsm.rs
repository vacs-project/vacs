//! Per-tap segmentation finite state machine.
//!
//! Pure logic; no I/O. Consumes [`ReplaySourceEvent`]s plus periodic [`Fsm::tick`] calls and
//! emits [`FsmAction`]s the recorder turns into file operations.
//!
//! # Behavior
//!
//! For each tap the FSM maintains one of two states:
//! - **Idle**: no clip open. `Frame` events are dropped.
//! - **Recording**: a clip is open. Frames are written. The clip closes after
//!   `hangover_ms` of silence following the last `RxEnd`, or rotates into a fresh clip
//!   once the open clip's duration exceeds `max_clip_duration_s`.
//!
//! `RxBegin` arriving while idle opens a clip. `RxBegin` while recording adds a
//! transmitter. `RxEnd` removes a transmitter; the clip stays open until the hangover
//! expires so trailing audio is captured.
//!
//! When [`RecordingMode::Mixed`] is set, every tap id is collapsed to
//! [`TapId::Merged`] so all activity feeds a single clip stream.

use crate::replay::source::ReplaySourceEvent;
use crate::replay::{RecordingMode, ReplayConfig, TapId, Transmitter};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use trackaudio::Frequency;

/// Side-effect requested by the FSM. The recorder is responsible for actually performing it.
#[derive(Debug, Clone)]
pub enum FsmAction {
    OpenClip {
        clip_id: u64,
        tap: TapId,
        sample_rate: u32,
        channels: u16,
        callsign: Option<String>,
        frequency: Option<Frequency>,
        started_at: SystemTime,
    },
    WriteFrame {
        clip_id: u64,
        samples: Arc<[f32]>,
    },
    CloseClip {
        clip_id: u64,
        ended_at: SystemTime,
        duration_ms: u64,
    },
}

#[derive(Debug)]
struct OpenClip {
    clip_id: u64,
    started_at: Instant,
    started_at_wall: SystemTime,
    last_audio_at: Instant,
    transmitters: HashSet<Transmitter>,
    primary_callsign: Option<String>,
    frequency: Option<Frequency>,
}

#[derive(Debug)]
struct TapState {
    sample_rate: u32,
    channels: u16,
    open: Option<OpenClip>,
}

/// Replay segmentation FSM.
#[derive(Debug)]
pub struct Fsm {
    mode: RecordingMode,
    hangover: Duration,
    max_clip_duration: Duration,
    taps: HashMap<TapId, TapState>,
    next_clip_id: u64,
}

impl Fsm {
    pub fn new(config: &ReplayConfig) -> Self {
        Self {
            mode: config.recording_mode,
            hangover: Duration::from_millis(config.hangover_ms),
            max_clip_duration: Duration::from_secs(config.max_clip_duration_s),
            taps: HashMap::new(),
            next_clip_id: 1,
        }
    }

    /// Drive the FSM with one source event.
    pub fn on_event(
        &mut self,
        event: ReplaySourceEvent,
        now: Instant,
        now_wall: SystemTime,
    ) -> Vec<FsmAction> {
        match event {
            ReplaySourceEvent::TapOpened {
                tap,
                sample_rate,
                channels,
            } => {
                let key = self.map_tap(tap);
                let entry = self.taps.entry(key).or_insert(TapState {
                    sample_rate,
                    channels,
                    open: None,
                });

                let mut actions = Vec::new();
                if entry.sample_rate != sample_rate || entry.channels != channels {
                    if let Some(action) = close_open_clip(entry, now, now_wall) {
                        actions.push(action);
                    }
                    entry.sample_rate = sample_rate;
                    entry.channels = channels;
                }

                actions
            }
            ReplaySourceEvent::TapClosed { tap } => {
                let key = self.map_tap(tap);

                let mut actions = Vec::new();
                if let Some(entry) = self.taps.get_mut(&key)
                    && let Some(action) = close_open_clip(entry, now, now_wall)
                {
                    actions.push(action);
                }

                self.taps.remove(&key);

                actions
            }
            ReplaySourceEvent::Frame {
                tap,
                samples,
                captured_at,
            } => {
                let key = self.map_tap(tap);
                let Some(entry) = self.taps.get_mut(&key) else {
                    return Vec::new();
                };
                let Some(open) = entry.open.as_mut() else {
                    return Vec::new();
                };

                // Only treat frames as "fresh audio" while at least one transmitter is
                // active. PipeWire delivers silent buffers continuously even after RX
                // ends; if we updated `last_audio_at` unconditionally, the hangover
                // would never elapse and the clip would stay open forever.
                if !open.transmitters.is_empty() {
                    open.last_audio_at = captured_at;
                }

                let mut actions = vec![FsmAction::WriteFrame {
                    clip_id: open.clip_id,
                    samples,
                }];
                if captured_at.duration_since(open.started_at) >= self.max_clip_duration {
                    let sample_rate = entry.sample_rate;
                    let channels = entry.channels;
                    let prev_callsign = open.primary_callsign.clone();
                    let prev_frequency = open.frequency;
                    let prev_transmitters = std::mem::take(&mut open.transmitters);

                    if let Some(action) = close_open_clip(entry, captured_at, now_wall) {
                        actions.push(action);
                    }

                    let clip_id = self.next_clip_id;
                    self.next_clip_id += 1;

                    entry.open = Some(OpenClip {
                        clip_id,
                        started_at: captured_at,
                        started_at_wall: now_wall,
                        last_audio_at: captured_at,
                        transmitters: prev_transmitters,
                        primary_callsign: prev_callsign.clone(),
                        frequency: prev_frequency,
                    });

                    actions.push(FsmAction::OpenClip {
                        clip_id,
                        tap: key,
                        sample_rate,
                        channels,
                        callsign: prev_callsign,
                        frequency: prev_frequency,
                        started_at: now_wall,
                    });
                }

                actions
            }
            ReplaySourceEvent::RxBegin {
                tap,
                callsign,
                frequency,
            } => {
                let key = self.map_tap(tap);
                let Some(entry) = self.taps.get_mut(&key) else {
                    return Vec::new();
                };
                if let Some(open) = entry.open.as_mut() {
                    open.transmitters.insert(Transmitter {
                        callsign,
                        frequency,
                    });
                    open.last_audio_at = now;
                    return Vec::new();
                }

                let clip_id = self.next_clip_id;
                self.next_clip_id += 1;

                let mut transmitters = HashSet::new();
                transmitters.insert(Transmitter {
                    callsign: callsign.clone(),
                    frequency,
                });

                entry.open = Some(OpenClip {
                    clip_id,
                    started_at: now,
                    started_at_wall: now_wall,
                    last_audio_at: now,
                    transmitters,
                    primary_callsign: Some(callsign.clone()),
                    frequency: Some(frequency),
                });

                vec![FsmAction::OpenClip {
                    clip_id,
                    tap: key,
                    sample_rate: entry.sample_rate,
                    channels: entry.channels,
                    callsign: Some(callsign),
                    frequency: Some(frequency),
                    started_at: now_wall,
                }]
            }
            ReplaySourceEvent::RxEnd {
                callsign,
                frequency,
                active_transmitters,
            } => {
                // Resolve the end across every open clip rather than only the tap the
                // event currently routes to: if the user toggles speaker/headset for
                // this frequency mid-transmission, RxBegin landed on one tap but RxEnd
                // arrives on another. Without this, the original tap's clip would keep
                // its transmitter set non-empty forever and the hangover would never
                // expire.
                let active: Option<HashSet<String>> =
                    active_transmitters.map(|v| v.into_iter().collect());
                for entry in self.taps.values_mut() {
                    let Some(open) = entry.open.as_mut() else {
                        continue;
                    };
                    open.transmitters.remove(&Transmitter {
                        callsign: callsign.clone(),
                        frequency,
                    });
                    if let Some(active) = active.as_ref() {
                        open.transmitters
                            .retain(|t| t.frequency != frequency || active.contains(&t.callsign));
                    }
                }
                Vec::new()
            }
        }
    }

    /// Close any clips whose hangover has expired. Call periodically (e.g. every 100 ms).
    pub fn tick(&mut self, now: Instant, now_wall: SystemTime) -> Vec<FsmAction> {
        let mut actions = Vec::new();
        for entry in self.taps.values_mut() {
            let should_close = entry.open.as_ref().is_some_and(|o| {
                o.transmitters.is_empty() && now >= o.last_audio_at + self.hangover
            });
            if should_close && let Some(action) = close_open_clip(entry, now, now_wall) {
                actions.push(action);
            }
        }
        actions
    }

    fn map_tap(&self, tap: TapId) -> TapId {
        match self.mode {
            RecordingMode::Mixed => TapId::Merged,
            RecordingMode::PerTap => tap,
        }
    }
}

fn close_open_clip(entry: &mut TapState, now: Instant, now_wall: SystemTime) -> Option<FsmAction> {
    let open = entry.open.take()?;
    let duration_ms = u64::try_from(now.saturating_duration_since(open.started_at).as_millis())
        .unwrap_or(u64::MAX);
    let ended_at = open
        .started_at_wall
        .checked_add(Duration::from_millis(duration_ms))
        .unwrap_or(now_wall);
    Some(FsmAction::CloseClip {
        clip_id: open.clip_id,
        ended_at,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: RecordingMode) -> ReplayConfig {
        ReplayConfig {
            enabled: true,
            max_clips: 10,
            hangover_ms: 500,
            max_clip_duration_s: 5,
            recording_mode: mode,
        }
    }

    fn open_tap(fsm: &mut Fsm, tap: TapId, now: Instant, wall: SystemTime) {
        let actions = fsm.on_event(
            ReplaySourceEvent::TapOpened {
                tap,
                sample_rate: 48_000,
                channels: 1,
            },
            now,
            wall,
        );
        assert!(actions.is_empty());
    }

    fn rx_begin(callsign: &str, freq: u64) -> ReplaySourceEvent {
        ReplaySourceEvent::RxBegin {
            tap: TapId::Headset,
            callsign: callsign.to_owned(),
            frequency: Frequency::from(freq),
        }
    }

    fn rx_end(callsign: &str, freq: u64, active: Option<Vec<String>>) -> ReplaySourceEvent {
        ReplaySourceEvent::RxEnd {
            callsign: callsign.to_owned(),
            frequency: Frequency::from(freq),
            active_transmitters: active,
        }
    }

    fn frame(tap: TapId, captured_at: Instant) -> ReplaySourceEvent {
        ReplaySourceEvent::Frame {
            tap,
            samples: Arc::from(vec![0.0_f32; 480]),
            captured_at,
        }
    }

    #[test]
    fn rx_cycle_opens_and_closes_clip_after_hangover() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);

        let opened = fsm.on_event(rx_begin("DLH123", 121_500_000), t0, w0);
        assert!(matches!(opened.as_slice(), [FsmAction::OpenClip { .. }]));

        let writes = fsm.on_event(
            frame(TapId::Headset, t0 + Duration::from_millis(20)),
            t0,
            w0,
        );
        assert!(matches!(writes.as_slice(), [FsmAction::WriteFrame { .. }]));

        let ended = fsm.on_event(
            rx_end("DLH123", 121_500_000, None),
            t0 + Duration::from_millis(100),
            w0,
        );
        assert!(ended.is_empty());

        let early = fsm.tick(t0 + Duration::from_millis(400), w0);
        assert!(early.is_empty());

        let closed = fsm.tick(t0 + Duration::from_millis(700), w0);
        assert!(matches!(closed.as_slice(), [FsmAction::CloseClip { .. }]));
    }

    #[test]
    fn frames_outside_recording_are_dropped() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        open_tap(&mut fsm, TapId::Headset, t0, SystemTime::UNIX_EPOCH);

        let actions = fsm.on_event(frame(TapId::Headset, t0), t0, SystemTime::UNIX_EPOCH);
        assert!(actions.is_empty());
    }

    #[test]
    fn unknown_station_rxbegin_is_ignored() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let actions = fsm.on_event(rx_begin("DLH123", 121_500_000), t0, SystemTime::UNIX_EPOCH);
        assert!(actions.is_empty());
    }

    #[test]
    fn second_transmitter_does_not_open_new_clip() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);

        let _ = fsm.on_event(rx_begin("DLH1", 121_500_000), t0, w0);
        let actions = fsm.on_event(
            rx_begin("DLH2", 121_500_000),
            t0 + Duration::from_millis(50),
            w0,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn clip_stays_open_while_any_transmitter_active() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);
        let _ = fsm.on_event(rx_begin("DLH1", 121_500_000), t0, w0);
        let _ = fsm.on_event(rx_begin("DLH2", 121_500_000), t0, w0);
        let _ = fsm.on_event(
            rx_end("DLH1", 121_500_000, None),
            t0 + Duration::from_millis(10),
            w0,
        );

        let actions = fsm.tick(t0 + Duration::from_millis(1_000), w0);
        assert!(actions.is_empty());

        let _ = fsm.on_event(
            rx_end("DLH2", 121_500_000, None),
            t0 + Duration::from_millis(1_010),
            w0,
        );
        let closed = fsm.tick(t0 + Duration::from_millis(2_000), w0);
        assert!(matches!(closed.as_slice(), [FsmAction::CloseClip { .. }]));
    }

    #[test]
    fn active_transmitters_replaces_set() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);
        let _ = fsm.on_event(rx_begin("DLH1", 121_500_000), t0, w0);
        let _ = fsm.on_event(rx_begin("DLH2", 121_500_000), t0, w0);
        let _ = fsm.on_event(
            rx_end("DLH1", 121_500_000, Some(vec![])),
            t0 + Duration::from_millis(10),
            w0,
        );
        let closed = fsm.tick(t0 + Duration::from_millis(700), w0);
        assert!(matches!(closed.as_slice(), [FsmAction::CloseClip { .. }]));
    }

    #[test]
    fn long_clip_rotates_at_max_duration() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);
        let _ = fsm.on_event(rx_begin("DLH1", 121_500_000), t0, w0);

        let early = fsm.on_event(frame(TapId::Headset, t0 + Duration::from_secs(1)), t0, w0);
        assert_eq!(early.len(), 1);

        let late = fsm.on_event(frame(TapId::Headset, t0 + Duration::from_secs(6)), t0, w0);
        assert_eq!(late.len(), 3);
        assert!(matches!(late[0], FsmAction::WriteFrame { .. }));
        assert!(matches!(late[1], FsmAction::CloseClip { .. }));
        assert!(matches!(late[2], FsmAction::OpenClip { .. }));
    }

    #[test]
    fn mixed_mode_collapses_to_merged() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::Mixed));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);
        open_tap(&mut fsm, TapId::Speaker, t0, w0);

        let actions = fsm.on_event(rx_begin("DLH1", 121_500_000), t0, w0);
        let [FsmAction::OpenClip { tap, .. }] = actions.as_slice() else {
            panic!("expected OpenClip");
        };
        assert_eq!(*tap, TapId::Merged);
    }

    #[test]
    fn station_closed_finalizes_open_clip() {
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);
        let _ = fsm.on_event(rx_begin("DLH1", 121_500_000), t0, w0);

        let closed = fsm.on_event(
            ReplaySourceEvent::TapClosed {
                tap: TapId::Headset,
            },
            t0 + Duration::from_millis(50),
            w0,
        );
        assert!(matches!(closed.as_slice(), [FsmAction::CloseClip { .. }]));

        let after = fsm.on_event(
            rx_begin("DLH2", 121_500_000),
            t0 + Duration::from_millis(100),
            w0,
        );
        assert!(after.is_empty());
    }

    #[test]
    fn rxbegin_within_hangover_keeps_same_clip() {
        // Real-world: a single transmitter "stutters", emitting a quick
        // RxEnd/RxBegin pair. The gap is shorter than `hangover_ms` so the
        // FSM should reattach to the existing OpenClip rather than rotating.
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);

        let opened = fsm.on_event(rx_begin("DLH123", 121_500_000), t0, w0);
        let [
            FsmAction::OpenClip {
                clip_id: first_id, ..
            },
        ] = opened.as_slice()
        else {
            panic!("expected OpenClip");
        };

        // Stutter: RxEnd then RxBegin 200 ms later (well under the 500 ms hangover).
        let _ = fsm.on_event(
            rx_end("DLH123", 121_500_000, None),
            t0 + Duration::from_millis(50),
            w0,
        );
        let resumed = fsm.on_event(
            rx_begin("DLH123", 121_500_000),
            t0 + Duration::from_millis(250),
            w0,
        );
        assert!(
            resumed.is_empty(),
            "RxBegin within hangover must not emit any actions: {resumed:?}"
        );

        // A tick during the original hangover window must not close the clip,
        // because the second RxBegin re-armed it.
        let mid = fsm.tick(t0 + Duration::from_millis(400), w0);
        assert!(mid.is_empty());

        // Final RxEnd, then wait out the full hangover. The clip closes with
        // the same id we first observed, proving it was a single continuous clip.
        let _ = fsm.on_event(
            rx_end("DLH123", 121_500_000, None),
            t0 + Duration::from_millis(500),
            w0,
        );
        let closed = fsm.tick(t0 + Duration::from_millis(1_100), w0);
        let [
            FsmAction::CloseClip {
                clip_id: closed_id, ..
            },
        ] = closed.as_slice()
        else {
            panic!("expected single CloseClip, got {closed:?}");
        };
        assert_eq!(*closed_id, *first_id, "stutter must not rotate the clip");
    }

    #[test]
    fn rxend_on_different_tap_still_closes_clip() {
        // Real-world: user toggles speaker/headset routing for a frequency
        // mid-transmission. RxBegin landed on Headset, but the source emits
        // RxEnd on Speaker because routing has flipped. The Headset clip must
        // still see its transmitter cleared so the hangover can fire.
        let mut fsm = Fsm::new(&cfg(RecordingMode::PerTap));
        let t0 = Instant::now();
        let w0 = SystemTime::UNIX_EPOCH;
        open_tap(&mut fsm, TapId::Headset, t0, w0);
        open_tap(&mut fsm, TapId::Speaker, t0, w0);

        let opened = fsm.on_event(rx_begin("DLH123", 121_500_000), t0, w0);
        assert!(matches!(opened.as_slice(), [FsmAction::OpenClip { .. }]));

        // RxEnd routes to Speaker after the user toggled routing mid-transmission.
        let _ = fsm.on_event(
            ReplaySourceEvent::RxEnd {
                callsign: "DLH123".into(),
                frequency: Frequency::from(121_500_000_u64),
                active_transmitters: None,
            },
            t0 + Duration::from_millis(100),
            w0,
        );

        // Frames keep flowing on the Headset tap (silent buffers), but with the
        // transmitter cleared the hangover must elapse and close the clip.
        for ms in [200_u64, 400, 600, 800] {
            let _ = fsm.on_event(
                frame(TapId::Headset, t0 + Duration::from_millis(ms)),
                t0,
                w0,
            );
        }
        let closed = fsm.tick(t0 + Duration::from_millis(800), w0);
        assert!(
            matches!(closed.as_slice(), [FsmAction::CloseClip { .. }]),
            "clip on Headset must close even though RxEnd routed to Speaker, got {closed:?}"
        );
    }
}
