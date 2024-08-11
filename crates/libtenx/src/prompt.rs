use std::path::PathBuf;

pub enum DocType {
    Ruskel,
    Text,
}

pub enum Contents {
    /// Unresolved content that should be read from a file
    Path(PathBuf),
    /// Unresolved content that will be resolved in accord with DocType.
    Unresolved(String),
    /// Resolved content that can be passed to the model.
    Resolved(String),
}

/// Reference material included in the prompt.
pub struct Docs {
    /// The type of documentation.
    pub ty: DocType,
    /// The name of the documentation.
    pub name: String,
    /// The contents of the help document. May be resolved lazily.
    pub contents: Option<String>,
}

/// Prompt is an abstract representation of a single prompt in a conversation with a model.
pub struct Prompt {
    /// Files to attach, but which the model can't edit
    pub attach_paths: Vec<PathBuf>,
    /// Editable paths
    pub edit_paths: Vec<PathBuf>,
    /// The user's prompt
    pub user_prompt: String,
    /// Included documentation
    pub docs: Vec<Docs>,
}
