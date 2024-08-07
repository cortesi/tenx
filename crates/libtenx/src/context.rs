use std::path::PathBuf;

/// Defines the initial context of a conversation. This defines which files are editable, plus which
/// files and documentation will be provided as context.
#[derive(Debug)]
pub struct Context {
    /// Files to attach, but which the model can't edit
    pub attach_paths: Vec<PathBuf>,
    /// Editable paths
    pub edit_paths: Vec<PathBuf>,
    /// The user's initial prompt
    pub user_prompt: String,
}

impl Context {
    pub(crate) fn new(
        edit_paths: Vec<PathBuf>,
        attach_paths: Vec<PathBuf>,
        user_prompt: String,
    ) -> Self {
        Context {
            edit_paths,
            attach_paths,
            user_prompt,
        }
    }
}
