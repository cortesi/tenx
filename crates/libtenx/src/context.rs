/*!
Traits and implementations for including immutable reference material in model interactions. Each
context provider implements the `ContextProvider` trait and can generate one or more ContextItems
which are included in prompts.
*/

use async_trait::async_trait;
use fs_err as fs;
use libruskel::Ruskel as LibRuskel;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::{Result, TenxError},
    exec::exec,
    session::Session,
};

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
pub trait ContextProvider {
    /// Retrieves the context items for this provider.
    fn context_items(
        &self,
        config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>>;

    /// Returns a human-readable representation of the context provider.
    fn human(&self) -> String;

    /// Refreshes the content of the context provider.
    async fn refresh(&mut self, config: &Config) -> Result<()>;

    async fn needs_refresh(&self, _config: &Config) -> bool {
        false
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// A context provider that generates Rust API documentation using Ruskel.
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

#[async_trait]
impl ContextProvider for Ruskel {
    fn context_items(
        &self,
        _config: &crate::config::Config,
        _session: &Session,
    ) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "ruskel".to_string(),
            source: self.name.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        format!("ruskel: {}", self.name)
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        let ruskel = LibRuskel::new(&self.name);
        self.content = ruskel
            .render(false, false, true)
            .map_err(|e| TenxError::Resolve(e.to_string()))?;
        Ok(())
    }

    async fn needs_refresh(&self, _config: &Config) -> bool {
        self.content.is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// A context provider that represents the project's file structure.
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

#[derive(Debug, Serialize, Deserialize, Clone)]
enum PathType {
    SinglePath(String),
    Pattern(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// A context provider that handles file paths, either single files or glob patterns.
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

#[async_trait]
impl ContextProvider for Path {
    fn context_items(
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

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        Ok(())
    }
}

/// A specification for reference material included in the prompt. This may be turned into actual
/// Context objects with the ContextProvider::contexts() method.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
/// A context provider that fetches content from a remote URL.
pub struct Url {
    name: String,
    url: String,
    content: String,
}

impl Url {
    pub(crate) fn new(url: String) -> Self {
        let name = if url.len() > 40 {
            format!("{}...", &url[..37])
        } else {
            url.clone()
        };

        Self {
            name,
            url,
            content: String::new(),
        }
    }
}

#[async_trait]
impl ContextProvider for Url {
    fn context_items(
        &self,
        _config: &crate::config::Config,
        _session: &Session,
    ) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "url".to_string(),
            source: self.url.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        format!("url: {}", self.name)
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        let client = reqwest::Client::new();
        self.content = client
            .get(&self.url)
            .send()
            .await
            .map_err(|e| TenxError::Resolve(e.to_string()))?
            .text()
            .await
            .map_err(|e| TenxError::Resolve(e.to_string()))?;
        Ok(())
    }

    async fn needs_refresh(&self, _config: &Config) -> bool {
        self.content.is_empty()
    }
}

/// A context provider that produces reference material for model interactions.
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
/// A context provider for raw text content.
pub struct Text {
    name: String,
    content: String,
}

impl Text {
    pub(crate) fn new(name: String, content: String) -> Self {
        Self { name, content }
    }
}

#[async_trait]
impl ContextProvider for Text {
    fn context_items(
        &self,
        _config: &crate::config::Config,
        _session: &Session,
    ) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "text".to_string(),
            source: self.name.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        let lines = self.content.lines().count();
        let chars = self.content.chars().count();
        format!("text: {} ({} lines, {} chars)", self.name, lines, chars)
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
/// A context provider that captures command output
pub struct Cmd {
    command: String,
    content: String,
}

impl Cmd {
    pub(crate) fn new(command: String) -> Self {
        Self {
            command,
            content: String::new(),
        }
    }
}

#[async_trait]
impl ContextProvider for Cmd {
    fn context_items(
        &self,
        _config: &crate::config::Config,
        _session: &Session,
    ) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "cmd".to_string(),
            source: self.command.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        format!("cmd: {}", self.command)
    }

    async fn refresh(&mut self, config: &Config) -> Result<()> {
        let (_, stdout, stderr) = exec(config.project_root(), &self.command)?;

        let mut content = String::new();
        let stdout = stdout.trim_end();
        if !stdout.is_empty() {
            content.push_str(stdout);
        }
        let stderr = stderr.trim_end();
        if !stderr.is_empty() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(stderr);
        }
        self.content = content;
        Ok(())
    }

    async fn needs_refresh(&self, _config: &Config) -> bool {
        self.content.is_empty()
    }
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

#[async_trait]
impl ContextProvider for Context {
    fn context_items(
        &self,
        config: &crate::config::Config,
        session: &Session,
    ) -> Result<Vec<ContextItem>> {
        match self {
            Context::Ruskel(r) => r.context_items(config, session),
            Context::Path(g) => g.context_items(config, session),
            Context::ProjectMap(p) => p.context_items(config, session),
            Context::Url(u) => u.context_items(config, session),
            Context::Text(t) => t.context_items(config, session),
            Context::Cmd(c) => c.context_items(config, session),
        }
    }

    fn human(&self) -> String {
        match self {
            Context::Ruskel(r) => r.human(),
            Context::Path(g) => g.human(),
            Context::ProjectMap(p) => p.human(),
            Context::Url(u) => u.human(),
            Context::Text(t) => t.human(),
            Context::Cmd(c) => c.human(),
        }
    }

    async fn refresh(&mut self, config: &Config) -> Result<()> {
        match self {
            Context::Ruskel(r) => r.refresh(config).await,
            Context::Path(g) => g.refresh(config).await,
            Context::ProjectMap(p) => p.refresh(config).await,
            Context::Url(u) => u.refresh(config).await,
            Context::Text(t) => t.refresh(config).await,
            Context::Cmd(c) => c.refresh(config).await,
        }
    }

    async fn needs_refresh(&self, config: &Config) -> bool {
        match self {
            Context::Ruskel(r) => r.needs_refresh(config).await,
            Context::Path(g) => g.needs_refresh(config).await,
            Context::ProjectMap(p) => p.needs_refresh(config).await,
            Context::Url(u) => u.needs_refresh(config).await,
            Context::Text(t) => t.needs_refresh(config).await,
            Context::Cmd(c) => c.needs_refresh(config).await,
        }
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
