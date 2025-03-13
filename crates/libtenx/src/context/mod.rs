/*!
Traits and implementations for including immutable reference material in model interactions. Each
context provider implements the `ContextProvider` trait and can generate one or more ContextItems
which are included in prompts.
*/

use enum_dispatch::enum_dispatch;

mod cmd;
mod manager;
mod path;
mod project_map;
mod ruskel;
mod text;
mod url;

pub use cmd::*;
pub use manager::*;
pub use path::*;
pub use project_map::*;
pub use ruskel::*;
pub use text::*;
pub use url::*;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{config::Config, error::Result, session::Session};

/// An individual context item.
#[derive(Debug, Serialize, Deserialize, Clone)]
/// Represents a single piece of context information to include in a prompt. Each ContextProvider
/// can provide multiple ContextItems.
pub struct ContextItem {
    /// The type of context.
    pub ty: String,
    /// The source of the context.
    pub source: String,
    /// The contents of the context.
    pub body: String,
}

impl Context {
    /// Returns true if both contexts have the same name and type.
    pub fn is_dupe(&self, other: &Self) -> bool {
        match (self, other) {
            (Context::Ruskel(a), Context::Ruskel(b)) => a.name == b.name,
            (Context::Path(a), Context::Path(b)) => match (&a.path_type, &b.path_type) {
                (PathType::SinglePath(a), PathType::SinglePath(b)) => a == b,
                (PathType::Pattern(a), PathType::Pattern(b)) => a == b,
                _ => false,
            },
            (Context::ProjectMap(_), Context::ProjectMap(_)) => true,
            (Context::Url(a), Context::Url(b)) => a.url == b.url,
            (Context::Text(a), Context::Text(b)) => a.name == b.name,
            _ => false,
        }
    }
}

/// A trait for context providers that can be used to generate context items for a prompt.
#[async_trait]
#[enum_dispatch(Context)]
pub trait ContextProvider {
    /// Retrieves the context items for this provider.
    fn context_items(&self, config: &Config, session: &Session) -> Result<Vec<ContextItem>>;

    /// Returns a human-readable representation of the context provider.
    fn human(&self) -> String;

    /// Refreshes the content of the context provider.
    async fn refresh(&mut self, config: &Config) -> Result<()>;

    async fn needs_refresh(&self, _config: &Config) -> bool {
        false
    }
}

/// A context provider that produces reference material for model interactions.
#[enum_dispatch]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Context {
    /// API documentation generated using Ruskel
    Ruskel(Ruskel),
    /// One or more files matched by a path or glob pattern
    Path(Path),
    /// A list of all files in the project
    ProjectMap(ProjectMap),
    /// Content fetched from a remote URL
    Url(Url),
    /// Raw text content provided directly
    Text(Text),
    /// Output from executing a command
    Cmd(Cmd),
}

impl Context {
    /// Creates a new Context for plain text content.
    pub fn new_text(name: &str, content: &str) -> Self {
        Context::Text(Text::new(name.to_string(), content.to_string()))
    }

    /// Creates a new Context for a Ruskel document.
    pub fn new_ruskel(name: &str) -> Self {
        Context::Ruskel(Ruskel::new(name.to_string()))
    }

    /// Creates a new Context for a glob pattern.
    pub fn new_path(config: &Config, pattern: &str) -> Result<Self> {
        Ok(Context::Path(Path::new(config, pattern.to_string())?))
    }

    /// Creates a new Context for the project map.
    pub fn new_project_map() -> Self {
        Context::ProjectMap(ProjectMap::new())
    }

    /// Creates a new Context for a URL.
    pub fn new_url(url: &str) -> Self {
        Context::Url(Url::new(url.to_string()))
    }

    /// Creates a new Context for a command.
    pub fn new_cmd(command: &str) -> Self {
        Context::Cmd(Cmd::new(command.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    use crate::{
        model::{DummyModel, Model, ModelProvider},
        session::Session,
        testutils::test_project,
    };

    use tempfile::tempdir;

    #[test]
    fn test_context_manager() {
        let mut manager = ContextManager::new();
        assert!(manager.is_empty());

        // Add a context
        let context1 = Context::new_text("test1", "content1");
        manager.add(context1.clone());
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.list()[0].human(), "text: test1 (1 lines, 8 chars)");

        // Add another context
        let context2 = Context::new_text("test2", "content2");
        manager.add(context2);
        assert_eq!(manager.len(), 2);

        // Add a duplicate context (should replace the first one)
        let context3 = Context::new_text("test1", "updated content");
        manager.add(context3);
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.list()[0].human(), "text: test1 (1 lines, 15 chars)");

        // Clear all contexts
        manager.clear();
        assert!(manager.is_empty());
    }

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

    #[test]
    fn test_cmd_context() {
        let rt = Runtime::new().unwrap();
        let test_project = test_project();
        test_project.create_file_tree(&["test.txt"]);
        let config = test_project.config;
        let session = Session::new(&config).unwrap();
        let cmd = "echo 'hello world' && echo 'error' >&2";
        let mut context = Context::new_cmd(cmd);

        // Initial state
        assert!(rt.block_on(async { context.needs_refresh(&config).await }));

        // After refresh
        rt.block_on(async { context.refresh(&config).await.unwrap() });

        let items = rt.block_on(async { context.context_items(&config, &session).unwrap() });
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].ty, "cmd");
        assert_eq!(items[0].source, cmd);
        assert_eq!(items[0].body, "hello world\nerror");

        assert_eq!(context.human(), format!("cmd: {}", cmd));
        assert!(!rt.block_on(async { context.needs_refresh(&config).await }));
    }

    #[test]
    fn test_file_context_outside_project_root() {
        let test_project = test_project();
        let outside_dir = tempdir().unwrap();
        let outside_file_path = outside_dir.path().join("outside.txt");
        std::fs::write(&outside_file_path, "Outside content").unwrap();

        // Use config with CWD set to project root
        let mut config = test_project.config.clone();
        config = config.with_cwd(test_project.tempdir.path().to_path_buf());

        // Create context and verify rendering when referencing file outside project root
        let mut session = Session::new(&config).unwrap();
        session.contexts.push(Context::Path(
            Path::new(&config, outside_file_path.to_str().unwrap().to_string()).unwrap(),
        ));

        let model = Model::Dummy(DummyModel::default());
        if let Model::Dummy(dummy) = model {
            let rendered = dummy.render(&config, &session).unwrap();
            assert!(rendered.contains("Outside content"));
        }
    }
}
