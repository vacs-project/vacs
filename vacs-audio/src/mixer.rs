use crate::sources::{AudioSource, AudioSourceId};
use ringbuf::producer::Producer;
use std::collections::HashMap;
use std::time::Duration;

pub type RemovedSourceProducer = ringbuf::HeapProd<Box<dyn AudioSource>>;

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
            sources: HashMap::new(),
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
