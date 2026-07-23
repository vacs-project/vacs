use crate::app::state::{AppStateInner, sealed};
use crate::audio::manager::AudioManagerHandle;

pub trait AppStateAudioExt: sealed::Sealed {
    fn audio_manager_handle(&self) -> AudioManagerHandle;
}

impl AppStateAudioExt for AppStateInner {
    fn audio_manager_handle(&self) -> AudioManagerHandle {
        self.audio_manager.clone()
    }
}
