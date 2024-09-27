use fs_err as fs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{Result, Session, TenxError};
use libruskel::Ruskel as LibRuskel;

/// An individual context item.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ContextItem {
    /// The type of documentation.
    pub ty: String,
    /// The name of the documentation.
    pub name: String,
    /// The contents of the help document.
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextType {
    Ruskel,
    File,
    Glob,
}

impl ToString for ContextType {
    fn to_string(&self) -> String {
        match self {
            ContextType::Ruskel => "Ruskel".to_string(),
            ContextType::File => "File".to_string(),
            ContextType::Glob => "Glob".to_string(),
        }
    }
}

pub trait ContextProvider {
    fn typ(&self) -> &ContextType;
    fn name(&self) -> &str;
    fn contexts(
        &self,
        config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>>;
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Ruskel {
    name: String,
    content: String,
}

impl Ruskel {
    pub fn new(name: String) -> Result<Self> {
        let ruskel = LibRuskel::new(&name);
        let content = ruskel
            .render(false, false, true)
            .map_err(|e| TenxError::Resolve(e.to_string()))?;
        Ok(Self { name, content })
    }
}

impl ContextProvider for Ruskel {
    fn typ(&self) -> &ContextType {
        &ContextType::Ruskel
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn contexts(
        &self,
        _config: &crate::config::Config,
        _session: &Session,
    ) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: self.typ().to_string(),
            name: self.name.clone(),
            body: self.content.clone(),
        }])
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct File {
    path: PathBuf,
}

impl File {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl ContextProvider for File {
    fn typ(&self) -> &ContextType {
        &ContextType::File
    }

    fn name(&self) -> &str {
        self.path.to_str().unwrap_or("")
    }

    fn contexts(
        &self,
        _config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>> {
        let abs_path = session.abspath(&self.path)?;
        let body = fs::read_to_string(&abs_path)?;
        Ok(vec![ContextItem {
            ty: self.typ().to_string(),
            name: self.name().to_string(),
            body,
        }])
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Glob {
    pattern: String,
}

impl Glob {
    pub fn new(pattern: String) -> Self {
        Self { pattern }
    }
}

impl ContextProvider for Glob {
    fn typ(&self) -> &ContextType {
        &ContextType::Glob
    }

    fn name(&self) -> &str {
        &self.pattern
    }

    fn contexts(
        &self,
        config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>> {
        let matched_files = session.match_files_with_glob(config, &self.pattern)?;
        let mut contexts = Vec::new();
        for file in matched_files {
            let abs_path = session.abspath(&file)?;
            let body = fs::read_to_string(&abs_path)?;
            contexts.push(ContextItem {
                ty: ContextType::File.to_string(),
                name: file.to_string_lossy().into_owned(),
                body,
            });
        }
        Ok(contexts)
    }
}

/// A specification for reference material included in the prompt.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ContextSpec {
    Ruskel(Ruskel),
    File(File),
    Glob(Glob),
}

impl ContextSpec {
    /// Creates a new Context for a file path.
    pub fn new_path(path: impl AsRef<Path>) -> Self {
        ContextSpec::File(File::new(path))
    }

    /// Creates a new Context for a Ruskel document.
    pub fn new_ruskel(name: String) -> Result<Self> {
        Ok(ContextSpec::Ruskel(Ruskel::new(name)?))
    }

    /// Creates a new Context for a glob pattern.
    pub fn new_glob(pattern: String) -> Self {
        ContextSpec::Glob(Glob::new(pattern))
    }
}

impl ContextProvider for ContextSpec {
    fn typ(&self) -> &ContextType {
        match self {
            ContextSpec::Ruskel(r) => r.typ(),
            ContextSpec::File(f) => f.typ(),
            ContextSpec::Glob(g) => g.typ(),
        }
    }

    fn name(&self) -> &str {
        match self {
            ContextSpec::Ruskel(r) => r.name(),
            ContextSpec::File(f) => f.name(),
            ContextSpec::Glob(g) => g.name(),
        }
    }

    fn contexts(
        &self,
        config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>> {
        match self {
            ContextSpec::Ruskel(r) => r.contexts(config, session),
            ContextSpec::File(f) => f.contexts(config, session),
            ContextSpec::Glob(g) => g.contexts(config, session),
        }
    }
}
