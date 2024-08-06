use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClaudeError {
    #[error("Failed to render query: {0}")]
    RenderError(String),

    #[error("Unknown error occurred")]
    Unknown,
}

pub type Result<T> = std::result::Result<T, ClaudeError>;
