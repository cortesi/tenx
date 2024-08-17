use serde::{Deserialize, Serialize};

/// PromptInput is an abstract representation of a single prompt in a conversation with a model.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PromptInput {
    /// The user's prompt
    pub user_prompt: String,
}
