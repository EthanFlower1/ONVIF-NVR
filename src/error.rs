use thiserror::Error;

use crate::device_manager::onvif_client::OnvifError;

#[derive(Error, Debug, Clone)]
pub enum Error {
    #[error("API error: {0}")]
    Api(String),

    #[error("ONVIF error: {0}")]
    Onvif(String),

    #[error("Recording error: {0}")]
    Recording(String),

    #[error("Streaming error: {0}")]
    Streaming(String),

    #[error("Capture error: {0}")]
    Capture(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("FFmpeg error: {0}")]
    FFmpeg(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Authorization error: {0}")]
    Authorization(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Service error: {0}")]
    Service(String),

    #[error("Camera error: {0}")]
    Camera(String),

    #[error("Generic error: {0}")]
    Generic(String),

    #[error("Other error: {0}")]
    Other(String),
}
