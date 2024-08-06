use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClaudeError {
    #[error("Failed to render query: {0}")]
    RenderError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No paths provided")]
    NoPathsProvided,

    #[error("Workspace error: {0}")]
    Workspace(String),
}

pub type Result<T> = std::result::Result<T, ClaudeError>;
