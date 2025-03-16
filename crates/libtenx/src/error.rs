use serde::{Deserialize, Serialize};
use state;
use thiserror::Error;

use crate::throttle::Throttle;

pub type Result<T> = std::result::Result<T, TenxError>;

#[derive(Error, Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum TenxError {
    #[error("config error: {0}")]
    Config(String),

    #[error("Path error: {0}")]
    Path(String),

    #[error("Io error: {0}")]
    Io(String),

    #[error("Model error: {0}")]
    Model(String),

    #[error("{msg}: {path}")]
    NotFound { msg: String, path: String },

    #[error("Error parsing response from model: {user}")]
    ResponseParse {
        /// A friendly error message for the user
        user: String,
        /// Detailed error information for the model
        model: String,
    },

    #[error("Error resolving context: {0}")]
    Resolve(String),

    #[error("{0}")]
    SessionStore(String),

    #[error("Internal error: {0}")]
    Internal(String),

    /// A patch error, which could cause a retry.
    #[error("Error applying patch: {user}")]
    Patch { user: String, model: String },

    /// A patch error, which could cause a retry.
    #[error("{name}: {user}")]
    Check {
        /// The name of the validator that failed
        name: String,
        /// An error to display to the user
        user: String,
        /// An error to the model, often the full tool output
        model: String,
    },

    /// An error that occurs when sending an event.
    #[error("Error sending event: {0}")]
    EventSend(String),

    /// Error executing a shell command
    #[error("Error executing command: {cmd}")]
    Exec { cmd: String, error: String },

    /// We've been throttled by the model, but we don't have a retry-after header.
    #[error("Throttled: {0}")]
    Throttle(Throttle),

    /// We've exceeded the max retries trying to send a request.
    #[error("Max retries exceeded: {0}")]
    MaxRetries(u64),
}

impl TenxError {
    /// Returns the model response if the error is retryable, otherwise None.
    pub fn should_retry(&self) -> Option<String> {
        match self {
            TenxError::Check { model, .. } => Some(model.to_string()),
            TenxError::Patch { model, .. } => Some(model.to_string()),
            TenxError::ResponseParse { model, .. } => Some(model.to_string()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TenxError {
    fn from(error: std::io::Error) -> Self {
        TenxError::Io(error.to_string())
    }
}

impl From<misanthropy::Error> for TenxError {
    fn from(error: misanthropy::Error) -> Self {
        match error {
            misanthropy::Error::RateLimitExceeded(_msg) => TenxError::Throttle(Throttle::Backoff),
            misanthropy::Error::ApiOverloaded(_msg) => TenxError::Throttle(Throttle::Backoff),
            _ => TenxError::Model(error.to_string()),
        }
    }
}

impl From<state::Error> for TenxError {
    fn from(e: state::Error) -> Self {
        match e {
            state::Error::Path(e) => TenxError::Path(e),
            state::Error::Io(e) => TenxError::Io(e),
            state::Error::NotFound { msg, path } => TenxError::NotFound { msg, path },
            state::Error::Internal(e) => TenxError::Internal(e),
            state::Error::Patch { user, model } => TenxError::Patch { user, model },
        }
    }
}
