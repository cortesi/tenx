use super::ContextItem;
use super::ContextProvider;
use crate::config::Config;
use crate::error::Result;
use crate::session::Session;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A context provider that represents the project's file structure.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectMap;

impl ProjectMap {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ContextProvider for ProjectMap {
    fn context_items(&self, config: &Config, _: &Session) -> Result<Vec<ContextItem>> {
        let files = config.project_files()?;
        let body = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(vec![ContextItem {
            ty: "project_map".to_string(),
            source: "project_map".to_string(),
            body,
        }])
    }

    fn human(&self) -> String {
        "project_map".to_string()
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{context::{Context, ContextProvider}, testutils::test_project};

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
        config.project.include = vec!["**/*.rs".to_string(), "**/Cargo.toml".to_string()];

        let context_spec = Context::new_project_map();
        let mut expected_files = vec!["src/main.rs", "src/lib.rs", "tests/test1.rs", "Cargo.toml"];
        expected_files.sort();

        if let Context::ProjectMap(map) = context_spec {
            let contexts = map.context_items(&config, &test_project.session).unwrap();
            assert_eq!(contexts.len(), 1);

            let context = &contexts[0];
            assert_eq!(context.ty, "project_map");
            assert_eq!(context.source, "project_map");

            let mut actual_files: Vec<_> = context.body.lines().collect();
            actual_files.sort();
            assert_eq!(actual_files, expected_files);
        } else {
            panic!("Expected ContextSpec::ProjectMap");
        }
    }
}