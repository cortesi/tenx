use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// PromptInput is an abstract representation of a single prompt in a conversation with a model.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PromptInput {
    /// Editable paths
    pub edit_paths: Vec<PathBuf>,
    /// The user's prompt
    pub user_prompt: String,
}
