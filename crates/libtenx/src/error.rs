use thiserror::Error;

pub type Result<T> = std::result::Result<T, TenxError>;

#[derive(Error, Debug)]
pub enum TenxError {
    #[error("Failed to render query: {0}")]
    Render(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No paths provided")]
    NoPathsProvided,

    #[error("Workspace error: {0}")]
    Workspace(String),

    #[error("Model error: {0}")]
    Model(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Error applying operation: {0}")]
    Operation(String),

    #[error("Resolution error: {0}")]
    Resolve(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<misanthropy::Error> for TenxError {
    fn from(error: misanthropy::Error) -> Self {
        TenxError::Model(error.to_string())
    }
}

