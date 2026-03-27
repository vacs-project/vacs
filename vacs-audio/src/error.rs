use cpal::{BuildStreamError, PlayStreamError, StreamError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Audio device is not available, check if it is plugged in properly")]
    DeviceNotAvailable,
    #[error("Unsupported audio configuration, try a different audio device")]
    UnsupportedConfig,
    #[error("Audio device is busy or access was denied")]
    DeviceBusyOrDenied,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<BuildStreamError> for AudioError {
    fn from(e: BuildStreamError) -> Self {
        use BuildStreamError::*;
        match e {
            DeviceNotAvailable => AudioError::DeviceNotAvailable,
            StreamConfigNotSupported | InvalidArgument => AudioError::UnsupportedConfig,
            StreamIdOverflow => AudioError::Other(anyhow::anyhow!("Stream ID overflow")),
            BackendSpecific { err } => match err.description.as_str() {
                "0x8889000A" => {
                    tracing::info!(
                        "Received WASAPI error 0x8889000A: device is busy or in use in exclusive mode"
                    );
                    AudioError::DeviceBusyOrDenied
                }
                description => {
                    tracing::warn!(?description, "Backend specific cpal build stream error");
                    AudioError::Other(anyhow::anyhow!(description.to_string()))
                }
            },
        }
    }
}

impl From<PlayStreamError> for AudioError {
    fn from(e: PlayStreamError) -> Self {
        use PlayStreamError::*;
        match e {
            DeviceNotAvailable => AudioError::DeviceNotAvailable,
            BackendSpecific { err } => {
                tracing::debug!(?err, "Backend specific cpal play stream error");
                AudioError::Other(anyhow::anyhow!(err.description))
            }
        }
    }
}

impl From<StreamError> for AudioError {
    fn from(e: StreamError) -> Self {
        use StreamError::*;
        match e {
            DeviceNotAvailable => AudioError::DeviceNotAvailable,
            StreamInvalidated => AudioError::DeviceNotAvailable,
            BufferUnderrun => {
                tracing::debug!("Audio buffer underrun");
                AudioError::Other(anyhow::anyhow!("Audio buffer underrun"))
            }
            BackendSpecific { err } => {
                tracing::debug!(?err, "Backend specific cpal stream error");
                AudioError::Other(anyhow::anyhow!(err.description))
            }
        }
    }
}
