use serde::{Deserialize, Serialize};

/// Prompt represents a single prompt in a conversation with a model.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Prompt {
    /// A prompt provided by the user
    User(String),
    /// A prompt automatically generated from errors
    Auto(String),
}

impl Prompt {
    /// Returns the underlying string of the prompt
    pub fn text(&self) -> &str {
        match self {
            Prompt::User(s) | Prompt::Auto(s) => s,
        }
    }
}

impl Default for Prompt {
    fn default() -> Self {
        Prompt::User(String::new())
    }
}
