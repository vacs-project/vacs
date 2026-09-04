use crate::sources::{AudioSource, AudioSourceId};
use ringbuf::producer::Producer;
use std::collections::HashMap;
use std::time::Duration;

pub type RemovedSourceProducer = ringbuf::HeapProd<Box<dyn AudioSource>>;

const SOURCE_CAPACITY: usize = 256;

#[derive(Default)]
pub struct Mixer {
    sources: HashMap<AudioSourceId, Box<dyn AudioSource>>,
    removed_sources: Option<RemovedSourceProducer>,
}

impl Mixer {
    /// Creates a mixer that hands removed sources to `removed_sources` instead
    /// of freeing them on the audio thread; the owner must drain that queue.
    pub fn with_deferred_drop(removed_sources: RemovedSourceProducer) -> Self {
        Self {
            sources: HashMap::with_capacity(SOURCE_CAPACITY),
            removed_sources: Some(removed_sources),
        }
    }

    /// Deallocating a source is not real-time safe; queue it for the non-RT
    /// side. A full queue falls back to dropping inline.
    fn defer_drop(&mut self, source: Box<dyn AudioSource>) {
        if let Some(removed_sources) = &mut self.removed_sources {
            let _ = removed_sources.try_push(source);
        }
    }

    pub fn mix(&mut self, output: &mut [f32]) {
        // Initialize the output buffer by writing EQUILIBRIUM to all of its samples. AudioSources will
        // add their own samples on top of this.
        output.fill(cpal::Sample::EQUILIBRIUM);

        // Mix all sources into the output buffer, adding their samples on top of the EQUILIBRIUM.
        for src in self.sources.values_mut() {
            src.mix_into(output);
        }

        // Clamp mixed samples to [-1.0, 1.0] to avoid clipping.
        for sample in output {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }

    pub fn add_source(&mut self, source_id: AudioSourceId, source: Box<dyn AudioSource>) {
        if let Some(replaced) = self.sources.insert(source_id, source) {
            self.defer_drop(replaced);
        }
    }

    pub fn remove_source(&mut self, source_id: AudioSourceId) {
        if let Some(removed) = self.sources.remove(&source_id) {
            self.defer_drop(removed);
        }
    }

    pub fn start_source(&mut self, source_id: AudioSourceId) {
        if let Some(source) = self.sources.get_mut(&source_id) {
            source.start();
        }
    }

    pub fn stop_source(&mut self, source_id: AudioSourceId) {
        if let Some(source) = self.sources.get_mut(&source_id) {
            source.stop();
        }
    }

    pub fn restart_source(&mut self, source_id: AudioSourceId) {
        if let Some(source) = self.sources.get_mut(&source_id) {
            source.restart();
        }
    }

    pub fn set_source_volume(&mut self, source_id: AudioSourceId, volume: f32) {
        if let Some(source) = self.sources.get_mut(&source_id) {
            source.set_volume(volume);
        }
    }

    pub fn skip_in_source(&mut self, source_id: AudioSourceId, duration: Duration) {
        if let Some(source) = self.sources.get_mut(&source_id) {
            source.skip(duration);
        }
    }

    pub fn rewind_in_source(&mut self, source_id: AudioSourceId, duration: Duration) {
        if let Some(source) = self.sources.get_mut(&source_id) {
            source.rewind(duration);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::AudioSource;
    use ringbuf::HeapRb;
    use ringbuf::consumer::Consumer;
    use ringbuf::traits::{Observer, Split};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Adds a constant to every sample and counts its own deallocation, so tests
    /// can tell a deferred drop from an inline one.
    struct ConstSource {
        value: f32,
        drops: Arc<AtomicUsize>,
    }

    impl ConstSource {
        fn boxed(value: f32, drops: &Arc<AtomicUsize>) -> Box<dyn AudioSource> {
            Box::new(Self {
                value,
                drops: drops.clone(),
            })
        }
    }

    impl Drop for ConstSource {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl AudioSource for ConstSource {
        fn mix_into(&mut self, output: &mut [f32]) {
            for sample in output {
                *sample += self.value;
            }
        }

        fn start(&mut self) {}
        fn stop(&mut self) {}
        fn set_volume(&mut self, _volume: f32) {}
        fn skip(&mut self, _duration: Duration) {}
        fn rewind(&mut self, _duration: Duration) {}
    }

    /// Produces samples for a single `mix_into` call and is silent afterwards.
    struct OneShotSource {
        played: bool,
    }

    impl AudioSource for OneShotSource {
        fn mix_into(&mut self, output: &mut [f32]) {
            if self.played {
                return;
            }
            self.played = true;
            for sample in output {
                *sample += 0.5;
            }
        }

        fn start(&mut self) {}
        fn stop(&mut self) {}
        fn set_volume(&mut self, _volume: f32) {}
        fn skip(&mut self, _duration: Duration) {}
        fn rewind(&mut self, _duration: Duration) {}
    }

    #[derive(Default)]
    struct Calls {
        start: usize,
        stop: usize,
        volume: Vec<f32>,
        skipped: Vec<Duration>,
        rewound: Vec<Duration>,
    }

    /// Records the control calls the mixer forwards to it.
    struct RecordingSource {
        calls: Arc<parking_lot::Mutex<Calls>>,
    }

    impl RecordingSource {
        fn boxed(calls: &Arc<parking_lot::Mutex<Calls>>) -> Box<dyn AudioSource> {
            Box::new(Self {
                calls: calls.clone(),
            })
        }
    }

    impl AudioSource for RecordingSource {
        fn mix_into(&mut self, _output: &mut [f32]) {}

        fn start(&mut self) {
            self.calls.lock().start += 1;
        }

        fn stop(&mut self) {
            self.calls.lock().stop += 1;
        }

        fn set_volume(&mut self, volume: f32) {
            self.calls.lock().volume.push(volume);
        }

        fn skip(&mut self, duration: Duration) {
            self.calls.lock().skipped.push(duration);
        }

        fn rewind(&mut self, duration: Duration) {
            self.calls.lock().rewound.push(duration);
        }
    }

    fn deferred(capacity: usize) -> (Mixer, ringbuf::HeapCons<Box<dyn AudioSource>>) {
        let (prod, cons) = HeapRb::<Box<dyn AudioSource>>::new(capacity).split();
        (Mixer::with_deferred_drop(prod), cons)
    }

    fn assert_all_close(output: &[f32], expected: f32) {
        for sample in output {
            assert!(
                (sample - expected).abs() < 1e-6,
                "expected {expected}, got {sample}"
            );
        }
    }

    #[test]
    fn a_single_source_passes_through() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut mixer = Mixer::default();
        mixer.add_source(1, ConstSource::boxed(0.25, &drops));

        // Pre-filled with stale samples the mixer has to overwrite first.
        let mut output = [7.0f32; 8];
        mixer.mix(&mut output);

        assert_all_close(&output, 0.25);
    }

    #[test]
    fn sources_sum_into_the_output() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut mixer = Mixer::default();
        mixer.add_source(1, ConstSource::boxed(0.1, &drops));
        mixer.add_source(2, ConstSource::boxed(0.2, &drops));
        mixer.add_source(3, ConstSource::boxed(-0.05, &drops));

        let mut output = [0.0f32; 4];
        mixer.mix(&mut output);

        assert_all_close(&output, 0.25);
    }

    #[test]
    fn the_mix_is_clamped_to_unity() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut mixer = Mixer::default();
        mixer.add_source(1, ConstSource::boxed(0.8, &drops));
        mixer.add_source(2, ConstSource::boxed(0.9, &drops));

        let mut output = [0.0f32; 4];
        mixer.mix(&mut output);
        assert_all_close(&output, 1.0);

        mixer.add_source(3, ConstSource::boxed(-3.0, &drops));
        mixer.mix(&mut output);
        assert_all_close(&output, -1.0);
    }

    #[test]
    fn mixing_without_sources_yields_equilibrium() {
        let mut mixer = Mixer::default();
        let mut output = [1.0f32; 4];
        mixer.mix(&mut output);

        assert_all_close(&output, cpal::Sample::EQUILIBRIUM);
    }

    #[test]
    fn remove_source_defers_the_drop() {
        let drops = Arc::new(AtomicUsize::new(0));
        let (mut mixer, mut removed) = deferred(4);
        mixer.add_source(1, ConstSource::boxed(0.5, &drops));

        mixer.remove_source(1);

        assert_eq!(drops.load(Ordering::SeqCst), 0, "freed on the audio thread");
        assert_eq!(removed.occupied_len(), 1);

        let mut output = [0.0f32; 4];
        mixer.mix(&mut output);
        assert_all_close(&output, 0.0);

        drop(removed.try_pop().expect("removed source in the ring"));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_full_ring_drops_the_source_inline() {
        let drops = Arc::new(AtomicUsize::new(0));
        let (mut mixer, removed) = deferred(1);
        mixer.add_source(1, ConstSource::boxed(0.1, &drops));
        mixer.add_source(2, ConstSource::boxed(0.2, &drops));

        mixer.remove_source(1);
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        mixer.remove_source(2);
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "the source that did not fit must be dropped inline"
        );
        assert_eq!(removed.occupied_len(), 1);
    }

    #[test]
    fn add_source_defers_the_replaced_source() {
        let drops = Arc::new(AtomicUsize::new(0));
        let (mut mixer, mut removed) = deferred(4);
        mixer.add_source(1, ConstSource::boxed(0.1, &drops));

        mixer.add_source(1, ConstSource::boxed(0.4, &drops));

        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(removed.occupied_len(), 1);

        let mut output = [0.0f32; 4];
        mixer.mix(&mut output);
        assert_all_close(&output, 0.4);

        drop(removed.try_pop().expect("replaced source in the ring"));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn without_deferred_drop_sources_are_freed_inline() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut mixer = Mixer::default();
        mixer.add_source(1, ConstSource::boxed(0.1, &drops));

        mixer.remove_source(1);

        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn removing_an_unknown_source_defers_nothing() {
        let drops = Arc::new(AtomicUsize::new(0));
        let (mut mixer, removed) = deferred(4);
        mixer.add_source(1, ConstSource::boxed(0.1, &drops));

        mixer.remove_source(42);

        assert!(removed.is_empty());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_finished_source_stays_registered_until_it_is_removed() {
        let (mut mixer, removed) = deferred(4);
        mixer.add_source(1, Box::new(OneShotSource { played: false }));

        let mut output = [0.0f32; 4];
        mixer.mix(&mut output);
        assert_all_close(&output, 0.5);

        // The mixer has no notion of a finished source; it only goes silent.
        mixer.mix(&mut output);
        assert_all_close(&output, 0.0);

        mixer.remove_source(1);
        assert_eq!(removed.occupied_len(), 1, "the source was still registered");
    }

    #[test]
    fn control_calls_reach_only_the_addressed_source() {
        let calls = Arc::new(parking_lot::Mutex::new(Calls::default()));
        let other = Arc::new(parking_lot::Mutex::new(Calls::default()));
        let mut mixer = Mixer::default();
        mixer.add_source(1, RecordingSource::boxed(&calls));
        mixer.add_source(2, RecordingSource::boxed(&other));

        mixer.start_source(1);
        mixer.stop_source(1);
        mixer.restart_source(1);
        mixer.set_source_volume(1, 0.75);
        mixer.skip_in_source(1, Duration::from_millis(20));
        mixer.rewind_in_source(1, Duration::from_millis(40));

        let calls = calls.lock();
        // `restart` defaults to a stop followed by a start.
        assert_eq!(calls.start, 2);
        assert_eq!(calls.stop, 2);
        assert_eq!(calls.volume, vec![0.75]);
        assert_eq!(calls.skipped, vec![Duration::from_millis(20)]);
        assert_eq!(calls.rewound, vec![Duration::from_millis(40)]);

        let other = other.lock();
        assert_eq!(other.start, 0);
        assert_eq!(other.stop, 0);
        assert!(other.volume.is_empty());
    }

    #[test]
    fn control_calls_for_an_unknown_source_are_ignored() {
        let mut mixer = Mixer::default();

        mixer.start_source(1);
        mixer.stop_source(1);
        mixer.restart_source(1);
        mixer.set_source_volume(1, 0.5);
        mixer.skip_in_source(1, Duration::from_millis(20));
        mixer.rewind_in_source(1, Duration::from_millis(20));
    }
}
