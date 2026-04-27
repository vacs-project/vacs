use crate::app::state::{AppStateInner, sealed};
use crate::replay::recorder::ReplayRecorderHandle;

pub trait AppStateReplayExt: sealed::Sealed {
    fn replay_recorder_handle(&self) -> ReplayRecorderHandle;
}

impl AppStateReplayExt for AppStateInner {
    fn replay_recorder_handle(&self) -> ReplayRecorderHandle {
        self.replay_recorder.clone()
    }
}
