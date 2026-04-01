use crate::app::state::{AppStateInner, sealed};
use crate::audio::manager::{AudioBackendHandle, AudioManagerHandle};

pub trait AppStateAudioExt: sealed::Sealed {
    fn audio_backend_handle(&self) -> AudioBackendHandle;
    fn audio_manager_handle(&self) -> AudioManagerHandle;
}

impl AppStateAudioExt for AppStateInner {
    fn audio_backend_handle(&self) -> AudioBackendHandle {
        self.audio_backend.clone()
    }

    fn audio_manager_handle(&self) -> AudioManagerHandle {
        self.audio_manager.clone()
    }
}
