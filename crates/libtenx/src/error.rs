use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClaudeError {
    #[error("Failed to render query: {0}")]
    RenderError(String),

    #[error("Unknown error occurred")]
    Unknown,

    #[error("No Cargo.toml file found")]
    CargoTomlNotFound,

    #[error("At least one edit path must be provided")]
    NoEditPaths,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ClaudeError>;
