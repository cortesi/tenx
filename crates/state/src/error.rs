use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Operation;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum Error {
    #[error("Path error: {0}")]
    Path(String),

    #[error("Io error: {0}")]
    Io(String),

    #[error("{msg}: {path}")]
    NotFound { msg: String, path: String },

    #[error("Internal error: {0}")]
    Internal(String),

    /// A patch error, which could cause a retry.
    #[error("Error applying patch: {user}")]
    Patch { user: String, model: String },
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error.to_string())
    }
}

/// Represents a patch operation failure with context about which operation failed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchFailure {
    /// The user-facing error message
    pub user: String,
    /// The model-facing error message (for AI context)
    pub model: String,
    /// The operation that failed
    pub operation: Operation,
}
