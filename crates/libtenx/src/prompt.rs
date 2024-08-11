use std::path::PathBuf;

use crate::{Result, TenxError};
use libruskel::Ruskel;

#[derive(Debug, Clone)]
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
    /// The contents of the help document.
    pub contents: Contents,
}

impl Docs {
    /// Resolves the contents of the documentation.
    pub fn resolve(&mut self) -> Result<()> {
        self.contents =
            match std::mem::replace(&mut self.contents, Contents::Resolved(String::new())) {
                Contents::Path(path) => {
                    Contents::Resolved(std::fs::read_to_string(path).map_err(TenxError::Io)?)
                }
                Contents::Unresolved(content) => match self.ty {
                    DocType::Ruskel => {
                        let ruskel = Ruskel::new(&content);
                        Contents::Resolved(
                            ruskel
                                .render(false, false)
                                .map_err(|e| TenxError::Resolve(e.to_string()))?,
                        )
                    }
                    DocType::Text => {
                        return Err(TenxError::Resolve(
                            "Cannot resolve unresolved Text content".to_string(),
                        ))
                    }
                },
                resolved @ Contents::Resolved(_) => resolved,
            };
        Ok(())
    }

    /// Converts a Docs to a string representation.
    pub fn to_string(&self) -> Result<String> {
        match &self.contents {
            Contents::Resolved(content) => Ok(content.clone()),
            _ => Err(TenxError::Parse("Unresolved doc content".to_string())),
        }
    }
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

