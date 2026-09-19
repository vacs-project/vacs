use crate::sources::{AudioSource, AudioSourceId};
use ringbuf::producer::Producer;
use std::collections::HashMap;
use std::time::Duration;

pub type Graveyard = ringbuf::HeapProd<Box<dyn AudioSource>>;

pub struct Mixer {
    sources: HashMap<AudioSourceId, Box<dyn AudioSource>>,
    graveyard: Graveyard,
}

impl Mixer {
    /// Removed sources go to `graveyard` so whoever drains it frees them, not the data
    /// callback that runs the removal.
    pub fn new(graveyard: Graveyard) -> Self {
        Self {
            sources: HashMap::with_capacity(16),
            graveyard,
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
        self.sources.insert(source_id, source);
    }

    pub fn remove_source(&mut self, source_id: AudioSourceId) {
        if let Some(source) = self.sources.remove(&source_id) {
            self.graveyard.try_push(source).ok();
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
    use ringbuf::HeapRb;
    use ringbuf::consumer::Consumer;
    use ringbuf::traits::Split;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    impl AudioSource for DropFlag {
        fn mix_into(&mut self, _output: &mut [f32]) {}
        fn start(&mut self) {}
        fn stop(&mut self) {}
        fn set_volume(&mut self, _volume: f32) {}
        fn skip(&mut self, _duration: Duration) {}
        fn rewind(&mut self, _duration: Duration) {}
    }

    #[test]
    fn removed_source_is_freed_by_the_graveyard_drain_not_by_the_mixer() {
        let (prod, mut cons) = HeapRb::<Box<dyn AudioSource>>::new(4).split();
        let mut mixer = Mixer::new(prod);
        let dropped = Arc::new(AtomicBool::new(false));
        mixer.add_source(7, Box::new(DropFlag(dropped.clone())));

        mixer.remove_source(7);
        assert!(!dropped.load(Ordering::SeqCst));

        let parked = cons
            .try_pop()
            .expect("the removed source is parked in the graveyard");
        assert!(!dropped.load(Ordering::SeqCst));
        drop(parked);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn full_graveyard_frees_the_source_in_place() {
        let (prod, _cons) = HeapRb::<Box<dyn AudioSource>>::new(1).split();
        let mut mixer = Mixer::new(prod);
        let first = Arc::new(AtomicBool::new(false));
        let second = Arc::new(AtomicBool::new(false));
        mixer.add_source(1, Box::new(DropFlag(first.clone())));
        mixer.add_source(2, Box::new(DropFlag(second.clone())));

        mixer.remove_source(1);
        mixer.remove_source(2);
        assert!(!first.load(Ordering::SeqCst));
        assert!(second.load(Ordering::SeqCst));
    }
}
