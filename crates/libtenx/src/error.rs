use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClaudeError {
    #[error("Failed to render query: {0}")]
    RenderError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cargo.toml not found")]
    CargoTomlNotFound,

    #[error("No paths provided")]
    NoPathsProvided,

    #[error("No common ancestor found for the provided paths")]
    NoCommonAncestor,
}

pub type Result<T> = std::result::Result<T, ClaudeError>;
