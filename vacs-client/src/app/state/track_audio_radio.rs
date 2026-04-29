use crate::app::state::{AppStateInner, sealed};
use crate::radio::track_audio::TrackAudioRadioHandle;

pub trait AppStateTrackAudioRadioExt: sealed::Sealed {
    fn track_audio_radio_handle(&self) -> TrackAudioRadioHandle;
}

impl AppStateTrackAudioRadioExt for AppStateInner {
    fn track_audio_radio_handle(&self) -> TrackAudioRadioHandle {
        self.track_audio_radio.clone()
    }
}
