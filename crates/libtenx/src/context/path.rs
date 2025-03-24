use super::ContextItem;
use super::ContextProvider;
use crate::config::Config;
use crate::error::Result;
use crate::session::Session;
use async_trait::async_trait;
use fs_err as fs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum PathType {
    SinglePath(String),
    Pattern(String),
}

/// A context provider that handles file paths, either single files or glob patterns.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Path {
    pub(crate) path_type: PathType,
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

#[async_trait]
impl ContextProvider for Path {
    fn context_items(&self, config: &Config, _session: &Session) -> Result<Vec<ContextItem>> {
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
                source: file.to_string_lossy().into_owned(),
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

    fn id(&self) -> String {
        match &self.path_type {
            PathType::SinglePath(path) => format!("single:{}", path),
            PathType::Pattern(pattern) => format!("pattern:{}", pattern),
        }
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context::{Context, ContextProvider},
        testutils::test_project,
    };

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

        let mut config = test_project.config.clone();
        config.project.include = vec!["**/*.rs".to_string()];

        let context_spec = Context::new_path(&config, "**/*.rs").unwrap();
        assert!(matches!(context_spec, Context::Path(_)));

        if let Context::Path(path) = context_spec {
            let contexts = path.context_items(&config, &test_project.session).unwrap();

            let mut expected_files = vec!["src/main.rs", "src/lib.rs", "tests/test1.rs"];
            expected_files.sort();

            let mut actual_files: Vec<_> = contexts.iter().map(|c| c.source.as_str()).collect();
            actual_files.sort();

            assert_eq!(actual_files, expected_files);

            for context in contexts {
                assert_eq!(context.ty, "file");
                assert_eq!(test_project.read(&context.source), context.body);
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

        let config = test_project.config.clone();
        let context_spec = Context::new_path(&config, "src/main.rs").unwrap();
        assert!(matches!(context_spec, Context::Path(_)));

        if let Context::Path(path) = context_spec {
            let contexts = path.context_items(&config, &test_project.session).unwrap();

            assert_eq!(contexts.len(), 1);
            let context = &contexts[0];
            assert_eq!(context.source, "src/main.rs");
            assert_eq!(context.ty, "file");
            assert_eq!(test_project.read(&context.source), context.body);
        } else {
            panic!("Expected ContextSpec::Path");
        }

        let mut config_in_src = test_project.config.clone();
        config_in_src = config_in_src.with_cwd(test_project.tempdir.path().join("src"));
        let context_spec = Context::new_path(&config_in_src, "./lib.rs").unwrap();
        assert!(matches!(context_spec, Context::Path(_)));

        if let Context::Path(path) = context_spec {
            let contexts = path
                .context_items(&config_in_src, &test_project.session)
                .unwrap();

            assert_eq!(contexts.len(), 1);
            let context = &contexts[0];
            assert_eq!(context.source, "src/lib.rs");
            assert_eq!(context.ty, "file");
            assert_eq!(test_project.read(&context.source), context.body);
        } else {
            panic!("Expected ContextSpec::Path");
        }
    }
}
