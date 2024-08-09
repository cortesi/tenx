use std::path::PathBuf;

/// Prompt is an abstract representation of a single prompt in a conversation with a model.
pub struct Prompt {
    /// Files to attach, but which the model can't edit
    pub attach_paths: Vec<PathBuf>,
    /// Editable paths
    pub edit_paths: Vec<PathBuf>,
    /// The user's prompt
    pub user_prompt: String,
}
