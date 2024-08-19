use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, TenxError>;

#[derive(Error, Debug)]
pub enum TenxError {
    #[error("Failed to render query: {0}")]
    Render(String),

    #[error("File IO error: {path}: {source}")]
    FileIo {
        source: std::io::Error,
        path: PathBuf,
    },

    #[error("Retry error: {user}")]
    Retry { user: String, model: String },

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
}

impl TenxError {
    /// Constructs a FileIo error from an IO error and a path-like argument.
    pub fn fio<P: AsRef<std::path::Path>>(err: std::io::Error, path: P) -> Self {
        TenxError::FileIo {
            source: err,
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl From<misanthropy::Error> for TenxError {
    fn from(error: misanthropy::Error) -> Self {
        TenxError::Model(error.to_string())
    }
}
