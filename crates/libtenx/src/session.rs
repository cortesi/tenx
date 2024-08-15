use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use colored::*;
use libruskel::Ruskel;
use serde::{Deserialize, Serialize};

use crate::{dialect::Dialect, model::Model, prompt::PromptInput, Result, TenxError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextType {
    Ruskel,
    File,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ContextData {
    /// Unresolved content that should be read from a file
    Path(PathBuf),
    /// Unresolved content that will be resolved in accord with DocType.
    Unresolved(String),
    /// Resolved content that can be passed to the model.
    Resolved(String),
}

/// Reference material included in the prompt.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Context {
    /// The type of documentation.
    pub ty: ContextType,
    /// The name of the documentation.
    pub name: String,
    /// The contents of the help document.
    pub data: ContextData,
}

impl Context {
    /// Resolves the contents of the documentation.
    pub fn resolve(&mut self) -> Result<()> {
        self.data = match std::mem::replace(&mut self.data, ContextData::Resolved(String::new())) {
            ContextData::Path(path) => {
                ContextData::Resolved(std::fs::read_to_string(path).map_err(TenxError::Io)?)
            }
            ContextData::Unresolved(content) => match self.ty {
                ContextType::Ruskel => {
                    let ruskel = Ruskel::new(&content);
                    ContextData::Resolved(
                        ruskel
                            .render(false, false)
                            .map_err(|e| TenxError::Resolve(e.to_string()))?,
                    )
                }
                ContextType::File => {
                    return Err(TenxError::Resolve(
                        "Cannot resolve unresolved Text content".to_string(),
                    ))
                }
            },
            resolved @ ContextData::Resolved(_) => resolved,
        };
        Ok(())
    }

    /// Converts a Docs to a string representation.
    pub fn to_string(&self) -> Result<String> {
        match &self.data {
            ContextData::Resolved(content) => Ok(content.clone()),
            _ => Err(TenxError::Parse("Unresolved doc content".to_string())),
        }
    }
}

/// The serializable state of Tenx, which persists between invocations.
#[derive(Debug, Deserialize, Serialize)]
pub struct Session {
    pub snapshot: HashMap<PathBuf, String>,
    pub working_directory: PathBuf,
    pub dialect: Dialect,
    pub model: Option<Model>,
    pub prompt_inputs: Vec<PromptInput>,
    pub context: Vec<Context>,
}

impl Session {
    /// Creates a new Context with the specified working directory and dialect.
    pub fn new<P: AsRef<Path>>(working_directory: P, dialect: Dialect, model: Model) -> Self {
        Self {
            snapshot: HashMap::new(),
            working_directory: working_directory.as_ref().to_path_buf(),
            model: Some(model),
            dialect,
            prompt_inputs: vec![],
            context: vec![],
        }
    }

    /// Adds a new context to the session.
    pub fn add_context(&mut self, context: Context) {
        self.context.push(context);
    }

    /// Pretty prints the Session information.
    pub fn pretty_print(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "{} {:?}\n",
            "Working Directory:".blue().bold(),
            self.working_directory
        ));

        output.push_str(&format!("{}\n", "Files in Snapshot:".blue().bold()));
        for path in self.snapshot.keys() {
            output.push_str(&format!("  - {:?}\n", path));
        }
        output.push('\n');

        output.push_str(&format!(
            "{} {:?}\n",
            "Dialect:".blue().bold(),
            self.dialect
        ));

        output.push_str(&format!("{}\n", "Context:".blue().bold()));
        for context in &self.context {
            output.push_str(&format!("  - {:?}: {}\n", context.ty, context.name));
        }

        output
    }
}
