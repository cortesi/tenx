use fs_err as fs;
use serde::{Deserialize, Serialize};

use crate::{config::Config, Result, Session, TenxError};
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
    Path,
}

pub trait ContextProvider {
    /// Returns the type of the context provider.
    fn typ(&self) -> &ContextType;

    /// Returns the name of the context provider.
    fn name(&self) -> &str;

    /// Retrieves the context items for this provider.
    fn contexts(
        &self,
        config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>>;

    /// Returns a human-readable representation of the context provider.
    fn human(&self) -> String;

    /// Counts the number of context items for this provider.
    fn count(&self, config: &crate::config::Config, session: &Session) -> Result<usize>;

    /// Refreshes the content of the context provider.
    fn refresh(&mut self) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Ruskel {
    name: String,
    content: String,
}

impl Ruskel {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            content: String::new(),
        }
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
            ty: "ruskel".to_string(),
            name: self.name.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        format!("ruskel: {}", self.name)
    }

    fn count(&self, _config: &crate::config::Config, _session: &Session) -> Result<usize> {
        Ok(1)
    }

    fn refresh(&mut self) -> Result<()> {
        let ruskel = LibRuskel::new(&self.name);
        self.content = ruskel
            .render(false, false, true)
            .map_err(|e| TenxError::Resolve(e.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum PathType {
    SinglePath(String),
    Pattern(String),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Path {
    path_type: PathType,
}

impl Path {
    pub(crate) fn new(config: &Config, pattern: String) -> Result<Self> {
        let path_type = if pattern.contains('*') {
            PathType::Pattern(pattern)
        } else {
            PathType::SinglePath(pattern)
        };
        Ok(Self { path_type })
    }
}

impl ContextProvider for Path {
    fn typ(&self) -> &ContextType {
        &ContextType::Path
    }

    fn name(&self) -> &str {
        match &self.path_type {
            PathType::SinglePath(path) => path,
            PathType::Pattern(pattern) => pattern,
        }
    }

    fn contexts(
        &self,
        config: &crate::config::Config,
        _session: &Session,
    ) -> Result<Vec<ContextItem>> {
        let matched_files = match &self.path_type {
            PathType::SinglePath(path) => vec![std::path::PathBuf::from(path)],
            PathType::Pattern(pattern) => config.match_files_with_glob(pattern)?,
        };
        let mut contexts = Vec::new();
        for file in matched_files {
            let abs_path = config.abspath(&file)?;
            let body = fs::read_to_string(&abs_path)?;
            contexts.push(ContextItem {
                ty: "file".to_string(),
                name: file.to_string_lossy().into_owned(),
                body,
            });
        }
        Ok(contexts)
    }

    fn human(&self) -> String {
        match &self.path_type {
            PathType::SinglePath(path) => path.to_string(),
            PathType::Pattern(pattern) => pattern.to_string(),
        }
    }

    fn count(&self, config: &crate::config::Config, _: &Session) -> Result<usize> {
        match &self.path_type {
            PathType::SinglePath(_) => Ok(1),
            PathType::Pattern(pattern) => {
                let matched_files = config.match_files_with_glob(pattern)?;
                Ok(matched_files.len())
            }
        }
    }

    fn refresh(&mut self) -> Result<()> {
        Ok(())
    }
}

/// A specification for reference material included in the prompt. This may be turned into actual
/// Context objects with the ContextProvider::contexts() method.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ContextSpec {
    Ruskel(Ruskel),
    Path(Path),
}

impl ContextSpec {
    /// Creates a new Context for a Ruskel document.
    pub fn new_ruskel(name: String) -> Self {
        ContextSpec::Ruskel(Ruskel::new(name))
    }

    /// Creates a new Context for a glob pattern.
    pub fn new_glob(config: &Config, pattern: String) -> Result<Self> {
        Ok(ContextSpec::Path(Path::new(config, pattern)?))
    }
}

impl ContextProvider for ContextSpec {
    fn typ(&self) -> &ContextType {
        match self {
            ContextSpec::Ruskel(r) => r.typ(),
            ContextSpec::Path(g) => g.typ(),
        }
    }

    fn name(&self) -> &str {
        match self {
            ContextSpec::Ruskel(r) => r.name(),
            ContextSpec::Path(g) => g.name(),
        }
    }

    fn contexts(
        &self,
        config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>> {
        match self {
            ContextSpec::Ruskel(r) => r.contexts(config, session),
            ContextSpec::Path(g) => g.contexts(config, session),
        }
    }

    fn human(&self) -> String {
        match self {
            ContextSpec::Ruskel(r) => r.human(),
            ContextSpec::Path(g) => g.human(),
        }
    }

    fn count(&self, config: &crate::config::Config, session: &Session) -> Result<usize> {
        match self {
            ContextSpec::Ruskel(r) => r.count(config, session),
            ContextSpec::Path(g) => g.count(config, session),
        }
    }

    fn refresh(&mut self) -> Result<()> {
        match self {
            ContextSpec::Ruskel(r) => r.refresh(),
            ContextSpec::Path(g) => g.refresh(),
        }
    }
}
