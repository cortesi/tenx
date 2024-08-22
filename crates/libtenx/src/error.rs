use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, TenxError>;

#[derive(Error, Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum TenxError {
    #[error("Failed to render query: {0}")]
    Render(String),

    // We want error to be serializable, so can't include the source untransformed
    #[error("File IO error: {path}: {src}")]
    FileIo { src: String, path: PathBuf },

    #[error("No paths provided")]
    NoPathsProvided,

    #[error("Workspace error: {0}")]
    Workspace(String),

    #[error("Model error: {0}")]
    Model(String),

    #[error("Error parsing response from model: {0}")]
    ResponseParse(String),

    #[error("Error applying change: {0}")]
    Change(String),

    #[error("Resolution error: {0}")]
    Resolve(String),

    #[error("Session store error: {0}")]
    SessionStore(String),

    #[error("Internal error: {0}")]
    Internal(String),

    /// A patch error, which could cause a retry.
    #[error("Error applying patch: {user}")]
    Patch { user: String, model: String },

    /// A patch error, which could cause a retry.
    #[error("Error applying {name} validation: {user}")]
    Validation {
        /// The name of the validator that failed
        name: String,
        /// An error to display to the user
        user: String,
        /// An error to the model, often the full tool output
        model: String,
    },
}

impl TenxError {
    /// Constructs a FileIo error from an IO error and a path-like argument.
    pub fn fio<P: AsRef<std::path::Path>>(err: std::io::Error, path: P) -> Self {
        TenxError::FileIo {
            src: err.to_string(),
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Returns the model response if the error is retryable, otherwise None.
    pub fn should_retry(&self) -> Option<&str> {
        match self {
            TenxError::Validation { model, .. } => Some(model),
            TenxError::Patch { model, .. } => Some(model),
            _ => None,
        }
    }
}

impl From<misanthropy::Error> for TenxError {
    fn from(error: misanthropy::Error) -> Self {
        TenxError::Model(error.to_string())
    }
}

