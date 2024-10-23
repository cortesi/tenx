use fs_err as fs;
use libruskel::Ruskel as LibRuskel;
use serde::{Deserialize, Serialize};

use crate::{config::Config, Result, Session, TenxError};

/// An individual context item.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ContextItem {
    /// The type of context.
    pub ty: String,
    /// The name of the context.
    pub name: String,
    /// The contents of the context.
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextType {
    Ruskel,
    Path,
    ProjectMap,
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
pub struct ProjectMap;

impl ProjectMap {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl ContextProvider for ProjectMap {
    fn typ(&self) -> &ContextType {
        &ContextType::ProjectMap
    }

    fn name(&self) -> &str {
        "project_map"
    }

    fn contexts(&self, config: &Config, _: &Session) -> Result<Vec<ContextItem>> {
        let files = config.included_files()?;
        let body = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(vec![ContextItem {
            ty: "project_map".to_string(),
            name: "project_map".to_string(),
            body,
        }])
    }

    fn human(&self) -> String {
        "project_map".to_string()
    }

    fn count(&self, config: &Config, _: &Session) -> Result<usize> {
        Ok(config.included_files()?.len())
    }

    fn refresh(&mut self) -> Result<()> {
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
        let pattern = config.normalize_path(pattern)?.display().to_string();
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
    ProjectMap(ProjectMap),
}

impl ContextSpec {
    /// Creates a new Context for a Ruskel document.
    pub fn new_ruskel(name: String) -> Self {
        ContextSpec::Ruskel(Ruskel::new(name))
    }

    /// Creates a new Context for a glob pattern.
    pub fn new_path(config: &Config, pattern: String) -> Result<Self> {
        Ok(ContextSpec::Path(Path::new(config, pattern)?))
    }

    /// Creates a new Context for the project map.
    pub fn new_project_map() -> Self {
        ContextSpec::ProjectMap(ProjectMap::new())
    }
}

impl ContextProvider for ContextSpec {
    fn typ(&self) -> &ContextType {
        match self {
            ContextSpec::Ruskel(r) => r.typ(),
            ContextSpec::Path(g) => g.typ(),
            ContextSpec::ProjectMap(p) => p.typ(),
        }
    }

    fn name(&self) -> &str {
        match self {
            ContextSpec::Ruskel(r) => r.name(),
            ContextSpec::Path(g) => g.name(),
            ContextSpec::ProjectMap(p) => p.name(),
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
            ContextSpec::ProjectMap(p) => p.contexts(config, session),
        }
    }

    fn human(&self) -> String {
        match self {
            ContextSpec::Ruskel(r) => r.human(),
            ContextSpec::Path(g) => g.human(),
            ContextSpec::ProjectMap(p) => p.human(),
        }
    }

    fn count(&self, config: &crate::config::Config, session: &Session) -> Result<usize> {
        match self {
            ContextSpec::Ruskel(r) => r.count(config, session),
            ContextSpec::Path(g) => g.count(config, session),
            ContextSpec::ProjectMap(p) => p.count(config, session),
        }
    }

    fn refresh(&mut self) -> Result<()> {
        match self {
            ContextSpec::Ruskel(r) => r.refresh(),
            ContextSpec::Path(g) => g.refresh(),
            ContextSpec::ProjectMap(p) => p.refresh(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Include;
    use crate::testutils::test_project;
    use tempfile::tempdir;

    #[test]
    fn test_project_map_context() {
        let test_project = test_project();
        test_project.create_file_tree(&[
            "src/main.rs",
            "src/lib.rs",
            "tests/test1.rs",
            "README.md",
            "Cargo.toml",
        ]);

        let mut config = test_project.config.clone();
        config.include = Include::Glob(vec!["**/*.rs".to_string(), "**/Cargo.toml".to_string()]);

        let context_spec = ContextSpec::new_project_map();
        let mut expected_files = vec!["src/main.rs", "src/lib.rs", "tests/test1.rs", "Cargo.toml"];
        expected_files.sort();

        if let ContextSpec::ProjectMap(map) = context_spec {
            let contexts = map.contexts(&config, &test_project.session).unwrap();
            assert_eq!(contexts.len(), 1);

            let context = &contexts[0];
            assert_eq!(context.ty, "project_map");
            assert_eq!(context.name, "project_map");

            let mut actual_files: Vec<_> = context.body.lines().collect();
            actual_files.sort();
            assert_eq!(actual_files, expected_files);
        } else {
            panic!("Expected ContextSpec::ProjectMap");
        }
    }

    #[test]
    fn test_glob_context_initialization() {
        let test_project = test_project();
        test_project.create_file_tree(&[
            "src/main.rs",
            "src/lib.rs",
            "tests/test1.rs",
            "README.md",
            "Cargo.toml",
        ]);

        // Set the include to use glob instead of git
        let mut config = test_project.config.clone();
        config.include = Include::Glob(vec!["**/*.rs".to_string()]);

        let context_spec = ContextSpec::new_path(&config, "**/*.rs".to_string()).unwrap();
        assert!(matches!(context_spec, ContextSpec::Path(_)));

        if let ContextSpec::Path(path) = context_spec {
            let contexts = path.contexts(&config, &test_project.session).unwrap();

            let mut expected_files = vec!["src/main.rs", "src/lib.rs", "tests/test1.rs"];
            expected_files.sort();

            let mut actual_files: Vec<_> = contexts.iter().map(|c| c.name.as_str()).collect();
            actual_files.sort();

            assert_eq!(actual_files, expected_files);

            for context in contexts {
                assert_eq!(context.ty, "file");
                assert_eq!(test_project.read(&context.name), context.body);
            }
        } else {
            panic!("Expected ContextSpec::Path");
        }
    }

    #[test]
    fn test_single_file_context_initialization() {
        let test_project = test_project();
        test_project.create_file_tree(&[
            "src/main.rs",
            "src/lib.rs",
            "tests/test1.rs",
            "README.md",
            "Cargo.toml",
        ]);

        // Test with absolute path from project root
        let config = test_project.config.clone();
        let context_spec = ContextSpec::new_path(&config, "src/main.rs".to_string()).unwrap();
        assert!(matches!(context_spec, ContextSpec::Path(_)));

        if let ContextSpec::Path(path) = context_spec {
            let contexts = path.contexts(&config, &test_project.session).unwrap();

            assert_eq!(contexts.len(), 1);
            let context = &contexts[0];
            assert_eq!(context.name, "src/main.rs");
            assert_eq!(context.ty, "file");
            assert_eq!(test_project.read(&context.name), context.body);
        } else {
            panic!("Expected ContextSpec::Path");
        }

        // Test with relative path from subdirectory
        let mut config_in_src = test_project.config.clone();
        config_in_src = config_in_src.with_test_cwd(test_project.tempdir.path().join("src"));
        let context_spec = ContextSpec::new_path(&config_in_src, "lib.rs".to_string()).unwrap();
        assert!(matches!(context_spec, ContextSpec::Path(_)));

        if let ContextSpec::Path(path) = context_spec {
            let contexts = path
                .contexts(&config_in_src, &test_project.session)
                .unwrap();

            assert_eq!(contexts.len(), 1);
            let context = &contexts[0];
            assert_eq!(context.name, "src/lib.rs");
            assert_eq!(context.ty, "file");
            assert_eq!(test_project.read(&context.name), context.body);
        } else {
            panic!("Expected ContextSpec::Path");
        }
    }

    #[test]
    fn test_file_context_outside_project_root() {
        let test_project = test_project();
        let outside_dir = tempdir().unwrap();
        let outside_file_path = outside_dir.path().join("outside.txt");
        std::fs::write(&outside_file_path, "Outside content").unwrap();

        // Test with absolute path
        let config = test_project.config.clone();
        let context_spec =
            ContextSpec::new_path(&config, outside_file_path.to_str().unwrap().to_string())
                .unwrap();
        assert!(matches!(context_spec, ContextSpec::Path(_)));

        if let ContextSpec::Path(path) = context_spec {
            let contexts = path.contexts(&config, &test_project.session).unwrap();

            assert_eq!(contexts.len(), 1);
            let context = &contexts[0];
            assert_eq!(context.name, outside_file_path.to_str().unwrap());
            assert_eq!(context.ty, "file");
            assert_eq!(context.body, "Outside content");
        } else {
            panic!("Expected ContextSpec::Path");
        }

        // Test with relative path
        let mut config_with_outside_cwd = config.clone();
        config_with_outside_cwd =
            config_with_outside_cwd.with_test_cwd(outside_dir.path().to_path_buf());
        let relative_context_spec =
            ContextSpec::new_path(&config_with_outside_cwd, "outside.txt".to_string()).unwrap();
        assert!(matches!(relative_context_spec, ContextSpec::Path(_)));

        if let ContextSpec::Path(path) = relative_context_spec {
            let contexts = path
                .contexts(&config_with_outside_cwd, &test_project.session)
                .unwrap();

            assert_eq!(contexts.len(), 1);
            let context = &contexts[0];
            assert_eq!(context.name, outside_file_path.to_str().unwrap());
            assert_eq!(context.ty, "file");
            assert_eq!(context.body, "Outside content");
        } else {
            panic!("Expected ContextSpec::Path");
        }
    }
}
