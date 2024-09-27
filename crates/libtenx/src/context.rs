use fs_err as fs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{Result, Session};

/// A specification for reference material included in the prompt.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Context {
    /// The type of documentation.
    pub ty: ContextType,
    /// The name of the documentation.
    pub name: String,
    /// The contents of the help document.
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextType {
    Ruskel,
    File,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ContextData {
    /// Unresolved content that should be read from a file each time the session is rendered.
    Path(PathBuf),
    /// Resolved content that can be passed to the model.
    String(String),
}

/// A specification for reference material included in the prompt.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ContextSpec {
    /// The type of documentation.
    pub ty: ContextType,
    /// The name of the documentation.
    pub name: String,
    /// The contents of the help document.
    pub data: ContextData,
}

pub trait ContextProvider {
    fn typ(&self) -> &ContextType;
    fn name(&self) -> &str;
    fn contexts(&self, session: &Session) -> Result<Vec<Context>>;
}

impl ContextSpec {
    /// Creates a new Context for a file path.
    pub fn new_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        ContextSpec {
            ty: ContextType::File,
            name: path.to_string_lossy().into_owned(),
            data: ContextData::Path(path),
        }
    }

    /// Creates a new Context for a Ruskel document.
    pub fn new_ruskel(name: String, content: String) -> Self {
        ContextSpec {
            ty: ContextType::Ruskel,
            name,
            data: ContextData::String(content),
        }
    }

    /// Converts a Docs to a string representation.
    pub fn body(&self, session: &Session) -> Result<String> {
        match &self.data {
            ContextData::String(content) => Ok(content.clone()),
            ContextData::Path(path) => Ok(fs::read_to_string(session.abspath(path)?)?),
        }
    }
}

impl ContextProvider for ContextSpec {
    fn typ(&self) -> &ContextType {
        &self.ty
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn contexts(&self, session: &Session) -> Result<Vec<Context>> {
        let body = self.body(session)?;
        Ok(vec![Context {
            ty: self.ty.clone(),
            name: self.name.clone(),
            body,
        }])
    }
}
