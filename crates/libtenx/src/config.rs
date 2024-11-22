use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    env, fs,
    path::{absolute, Path, PathBuf},
    process::Command,
};

use globset::{Glob, GlobSetBuilder};
use normalize_path::NormalizePath;
use pathdiff::diff_paths;
use serde::ser::SerializeStruct;

use toml;

use crate::{checks::builtin_validators, checks::Check, dialect, model, Result, TenxError};

pub const HOME_CONFIG_FILE: &str = "tenx.toml";
pub const LOCAL_CONFIG_FILE: &str = ".tenx.toml";
const DEFAULT_RETRY_LIMIT: usize = 16;

const ANTHROPIC_API_KEY: &str = "ANTHROPIC_API_KEY";
const ANTHROPIC_CLAUDE_SONNET: &str = "claude-3-5-sonnet-latest";
const ANTHROPIC_CLAUDE_HAIKU: &str = "claude-3-5-haiku-latest";

const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const OPENAI_GPT_O1_PREVIEW: &str = "o1-preview";
const OPENAI_GPT_O1_MINI: &str = "o1-mini";
const OPENAI_GPT4O: &str = "gpt-4o";
const OPENAI_GPT4O_MINI: &str = "gpt-4o-mini";

const DEEPINFRA_API_KEY: &str = "DEEPINFRA_API_KEY";
const DEEPINFRA_API_BASE: &str = "https://api.deepinfra.com/v1/openai";

const XAI_API_KEY: &str = "XAI_API_KEY";
const XAI_API_BASE: &str = "https://api.x.ai/v1";
const XAI_DEFAULT_GROK: &str = "grok-beta";

const GOOGLEAI_API_KEY: &str = "GOOGLEAI_API_KEY";
const GOOGLEAI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/openai";
const GOOGLEAI_GEMINI_EXP: &str = "gemini-exp-1114";

macro_rules! serialize_if_different {
    ($state:expr, $self:expr, $default:expr, $field:ident) => {
        if $self.full || $self.$field != $default.$field {
            $state.serialize_field(stringify!($field), &$self.$field)?;
        }
    };
}

fn default_retry_limit() -> usize {
    DEFAULT_RETRY_LIMIT
}

fn is_relative<P: AsRef<Path>>(path: P) -> bool {
    let path_str = path.as_ref().to_str().unwrap_or("");
    path_str.starts_with("./") || path_str.starts_with("../")
}

/// Returns the path to the configuration directory.
pub fn home_config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".config")
        .join("tenx")
}

/// Recursively walk a directory and collect files that match a glob pattern.
/// Returns paths relative to the root directory.
fn walk_directory(
    root: &Path,
    current_dir: &Path,
    globset: &globset::GlobSet,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(current_dir).map_err(|e| TenxError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| TenxError::Io(e.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_directory(root, &path, globset, files)?;
        } else {
            let relative_path = path
                .strip_prefix(root)
                .map_err(|e| TenxError::Internal(format!("Path not under root: {}", e)))?;
            if globset.is_match(relative_path) {
                files.push(relative_path.to_path_buf());
            }
        }
    }
    Ok(())
}

/// Finds the root directory based on a specified working directory, git repo root, or .tenx.conf
/// file.
fn find_project_root(current_dir: &Path) -> PathBuf {
    let mut dir = current_dir.to_path_buf();
    loop {
        if dir.join(".git").is_dir() || dir.join(LOCAL_CONFIG_FILE).is_file() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    current_dir.to_path_buf()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct DefaultContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ruskel: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    #[serde(default = "default_project_map")]
    pub project_map: bool,
}

impl Default for DefaultContext {
    fn default() -> Self {
        Self {
            ruskel: Vec::new(),
            path: Vec::new(),
            project_map: true,
        }
    }
}

fn default_project_map() -> bool {
    true
}

/// Configuration for an Anthropic model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeConf {
    /// The name of the model.
    pub name: String,
    /// The API model identifier.
    pub api_model: String,
    /// The API key.
    pub key: String,
    /// The environment variable to load the API key from.
    pub key_env: String,
}

impl ClaudeConf {
    /// Loads API key from environment if key is empty and key_env is specified
    pub fn load_env(mut self) -> Self {
        if self.key.is_empty() && !self.key_env.is_empty() {
            if let Ok(key) = env::var(&self.key_env) {
                self.key = key;
            }
        }
        self
    }

    /// Returns a string representation of the model configuration.
    pub fn text_config(&self, verbose: bool) -> String {
        let key = if verbose {
            self.key.clone()
        } else {
            ModelConfig::abbreviate_key(&self.key)
        };
        [
            format!("api_model = {}", self.api_model),
            format!("key = {}", key),
            format!("key_env = {}", self.key_env),
        ]
        .join("\n")
    }

    /// Converts ClaudeConf to a Claude model.
    pub fn to_model(&self, no_stream: bool) -> Result<model::Claude> {
        if self.api_model.is_empty() {
            return Err(TenxError::Model("Empty API model name".into()));
        }
        if self.key.is_empty() {
            return Err(TenxError::Model("Empty Anthropic API key".into()));
        }
        Ok(model::Claude {
            api_model: self.api_model.clone(),
            anthropic_key: self.key.clone(),
            streaming: !no_stream,
        })
    }
}

/// Configuration for an OpenAI model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiConf {
    /// The name of the model.
    pub name: String,
    /// The API model identifier.
    pub api_model: String,
    /// The API key.
    pub key: String,
    /// The environment variable to load the API key from.
    pub key_env: String,
    /// The base URL for the API.
    pub api_base: String,
    /// Whether the model can stream responses.
    pub can_stream: bool,
    /// Whether the model supports a separate system prompt.
    pub no_system_prompt: bool,
}

impl OpenAiConf {
    /// Loads API key from environment if key is empty and key_env is specified
    pub fn load_env(mut self) -> Self {
        if self.key.is_empty() && !self.key_env.is_empty() {
            if let Ok(key) = env::var(&self.key_env) {
                self.key = key;
            }
        }
        self
    }

    /// Returns a string representation of the model configuration.
    pub fn text_config(&self, verbose: bool) -> String {
        let key = if verbose {
            self.key.clone()
        } else {
            ModelConfig::abbreviate_key(&self.key)
        };
        [
            format!("api_base = {}", self.api_base),
            format!("api_model = {}", self.api_model),
            format!("key = {}", key),
            format!("key_env = {}", self.key_env),
            format!("no_system_prompt = {}", self.no_system_prompt),
            format!("stream = {}", self.can_stream),
        ]
        .join("\n")
    }

    /// Converts OpenAiConf to an OpenAi model.
    pub fn to_model(&self, no_stream: bool) -> model::OpenAi {
        model::OpenAi {
            api_model: self.api_model.clone(),
            openai_key: self.key.clone(),
            api_base: self.api_base.clone(),
            streaming: self.can_stream && !no_stream,
            no_system_prompt: self.no_system_prompt,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelConfig {
    Claude(ClaudeConf),
    OpenAi(OpenAiConf),
}

impl ModelConfig {
    /// Loads API key from environment if key is empty and key_env is specified
    pub fn load_env(self) -> Self {
        match self {
            ModelConfig::Claude(conf) => ModelConfig::Claude(conf.load_env()),
            ModelConfig::OpenAi(conf) => ModelConfig::OpenAi(conf.load_env()),
        }
    }

    /// Returns the name of the configured model.
    pub fn name(&self) -> &str {
        match self {
            ModelConfig::Claude(conf) => &conf.name,
            ModelConfig::OpenAi(conf) => &conf.name,
        }
    }

    /// Returns the kind of model (e.g. "claude").
    pub fn kind(&self) -> &'static str {
        match self {
            ModelConfig::Claude(_) => "claude",
            ModelConfig::OpenAi(_) => "openai",
        }
    }

    fn abbreviate_key(key: &str) -> String {
        if key.len() < 8 {
            key.to_string()
        } else {
            format!("{}...{}", &key[..4], &key[key.len() - 4..])
        }
    }

    /// Returns a string representation of the model configuration.
    pub fn text_config(&self, verbose: bool) -> String {
        match self {
            ModelConfig::Claude(conf) => conf.text_config(verbose),
            ModelConfig::OpenAi(conf) => conf.text_config(verbose),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigDialect {
    #[default]
    Tags,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ops {
    /// Allow the model to request to edit files in the project map
    pub edit: bool,
}

impl Default for Ops {
    fn default() -> Self {
        Self { edit: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tags {
    /// EXPERIMENTAL: enable smart change type
    pub smart: bool,
    /// Enable replace change type
    pub replace: bool,
    /// EXPERIMENTAL: enable udiff change type
    pub udiff: bool,
}

impl Default for Tags {
    fn default() -> Self {
        Self {
            smart: false,
            replace: true,
            udiff: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Include {
    #[default]
    Git,
    Glob(Vec<String>),
}

impl std::fmt::Display for Include {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Include::Git => write!(f, "git"),
            Include::Glob(patterns) => {
                write!(f, "glob patterns:")?;
                for pattern in patterns {
                    write!(f, " {}", pattern)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checks {
    pub rust_cargo_check: bool,
    pub rust_cargo_test: bool,
    pub rust_cargo_clippy: bool,
    pub python_ruff_check: bool,
}

impl Default for Checks {
    fn default() -> Self {
        Self {
            rust_cargo_check: true,
            rust_cargo_test: true,
            rust_cargo_clippy: false,
            python_ruff_check: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Formatters {
    pub rust_cargo_fmt: bool,
    pub python_ruff_fmt: bool,
}

impl Default for Formatters {
    fn default() -> Self {
        Self {
            rust_cargo_fmt: true,
            python_ruff_fmt: true,
        }
    }
}

// Note that we can't use Optional values in the config. TOML includes no way to render
// optional values, so our strategy of rendering the full config with a default config for
// documentation falls by the wayside.

#[derive(Debug, Clone, Deserialize)]
/// Configuration for the Tenx application.
pub struct Config {
    /// Available model configurations
    #[serde(default)]
    pub models: Vec<ModelConfig>,

    /// Disable streaming for all models
    #[serde(default)]
    pub no_stream: bool,

    /// The default dialect.
    #[serde(default)]
    pub default_dialect: ConfigDialect,

    /// Which files are included by default
    ///
    /// TOML examples:
    /// ```toml
    /// # Default Git include
    /// include = "git"
    ///
    /// # Glob include
    /// include = { glob = ["*.rs", "*.toml"] }
    /// ```
    #[serde(default)]
    pub include: Include,

    /// Glob patterns to exclude from the file list
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// Skip the preflight check.
    #[serde(default)]
    pub no_preflight: bool,

    /// The directory to store session state.
    #[serde(default)]
    /// The directory to store session state. Defaults to ~/.config/tenx/state
    pub session_store_dir: PathBuf,

    /// The number of times to retry a request.
    #[serde(default = "default_retry_limit")]
    pub retry_limit: usize,

    /// The tags dialect configuration.
    #[serde(default)]
    pub tags: Tags,

    /// Operations that can be executed by the model.
    #[serde(default)]
    pub ops: Ops,

    /// The default context configuration.
    #[serde(default)]
    pub default_context: DefaultContext,

    /// The name of the default model to use
    #[serde(default)]
    pub default_model: Option<String>,

    /// Check configuration.
    #[serde(default)]
    pub checks: Checks,

    /// Formatting configuration.
    #[serde(default)]
    pub formatters: Formatters,

    /// Project root configuration.
    #[serde(default)]
    pub project_root: ProjectRoot,

    /// Set a dummy model for end-to-end testing. Over-rides the configured model.
    #[serde(skip)]
    dummy_model: Option<model::DummyModel>,

    /// Set a dummy dialect for end-to-end testing. Over-rides the configured dialect.
    #[serde(skip)]
    dummy_dialect: Option<dialect::DummyDialect>,

    /// When true, serializes all fields regardless of default values.
    #[serde(skip)]
    full: bool,

    /// The current working directory when testing. We need this, because we can't change the CWD
    /// reliably in tests for reasons of concurrency.
    #[serde(skip)]
    test_cwd: Option<String>,
}

impl Serialize for Config {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let default = Config::default();
        let mut state = serializer.serialize_struct("Config", 11)?;
        serialize_if_different!(state, self, default, include);
        serialize_if_different!(state, self, default, models);
        serialize_if_different!(state, self, default, session_store_dir);
        serialize_if_different!(state, self, default, retry_limit);
        serialize_if_different!(state, self, default, no_preflight);
        serialize_if_different!(state, self, default, default_dialect);
        serialize_if_different!(state, self, default, tags);
        serialize_if_different!(state, self, default, ops);
        serialize_if_different!(state, self, default, default_context);
        serialize_if_different!(state, self, default, checks);
        serialize_if_different!(state, self, default, formatters);
        serialize_if_different!(state, self, default, project_root);
        state.end()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ProjectRoot {
    #[default]
    Discover,
    Path(PathBuf),
}

impl Serialize for ProjectRoot {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ProjectRoot::Discover => serializer.serialize_str(""),
            ProjectRoot::Path(path) => path.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ProjectRoot {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.is_empty() {
            Ok(ProjectRoot::Discover)
        } else {
            Ok(ProjectRoot::Path(PathBuf::from(s)))
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut models = Vec::new();

        if env::var(ANTHROPIC_API_KEY).is_ok() {
            models.extend_from_slice(&[
                ModelConfig::Claude(ClaudeConf {
                    name: "sonnet".to_string(),
                    api_model: ANTHROPIC_CLAUDE_SONNET.to_string(),
                    key: "".to_string(),
                    key_env: ANTHROPIC_API_KEY.to_string(),
                }),
                ModelConfig::Claude(ClaudeConf {
                    name: "haiku".to_string(),
                    api_model: ANTHROPIC_CLAUDE_HAIKU.to_string(),
                    key: "".to_string(),
                    key_env: ANTHROPIC_API_KEY.to_string(),
                }),
            ]);
        }

        if env::var(DEEPINFRA_API_KEY).is_ok() {
            models.push(ModelConfig::OpenAi(OpenAiConf {
                name: "qwen-coder".to_string(),
                api_model: "Qwen/Qwen2.5-Coder-32B-Instruct".to_string(),
                key: "".to_string(),
                key_env: DEEPINFRA_API_KEY.to_string(),
                api_base: DEEPINFRA_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
            }));
        }

        if env::var(OPENAI_API_KEY).is_ok() {
            models.extend_from_slice(&[
                ModelConfig::OpenAi(OpenAiConf {
                    name: "o1".to_string(),
                    api_model: OPENAI_GPT_O1_PREVIEW.to_string(),
                    key: "".to_string(),
                    key_env: OPENAI_API_KEY.to_string(),
                    api_base: crate::model::OPENAI_API_BASE.to_string(),
                    can_stream: false,
                    no_system_prompt: true,
                }),
                ModelConfig::OpenAi(OpenAiConf {
                    name: "o1-mini".to_string(),
                    api_model: OPENAI_GPT_O1_MINI.to_string(),
                    key: "".to_string(),
                    key_env: OPENAI_API_KEY.to_string(),
                    api_base: crate::model::openai::OPENAI_API_BASE.to_string(),
                    can_stream: false,
                    no_system_prompt: true,
                }),
                ModelConfig::OpenAi(OpenAiConf {
                    name: "gpt4o".to_string(),
                    api_model: OPENAI_GPT4O.to_string(),
                    key: "".to_string(),
                    key_env: OPENAI_API_KEY.to_string(),
                    api_base: crate::model::openai::OPENAI_API_BASE.to_string(),
                    can_stream: true,
                    no_system_prompt: false,
                }),
                ModelConfig::OpenAi(OpenAiConf {
                    name: "gpt4o-mini".to_string(),
                    api_model: OPENAI_GPT4O_MINI.to_string(),
                    key: "".to_string(),
                    key_env: OPENAI_API_KEY.to_string(),
                    api_base: crate::model::openai::OPENAI_API_BASE.to_string(),
                    can_stream: true,
                    no_system_prompt: false,
                }),
            ]);
        }

        if env::var(XAI_API_KEY).is_ok() {
            models.push(ModelConfig::OpenAi(OpenAiConf {
                name: "grok".to_string(),
                api_model: XAI_DEFAULT_GROK.to_string(),
                key: "".to_string(),
                key_env: XAI_API_KEY.to_string(),
                api_base: XAI_API_BASE.to_string(),
                can_stream: true,
                no_system_prompt: false,
            }));
        }

        if env::var(GOOGLEAI_API_KEY).is_ok() {
            models.push(ModelConfig::OpenAi(OpenAiConf {
                name: "gemini".to_string(),
                api_model: GOOGLEAI_GEMINI_EXP.to_string(),
                key: "".to_string(),
                key_env: GOOGLEAI_API_KEY.to_string(),
                api_base: GOOGLEAI_API_BASE.to_string(),
                can_stream: false,
                no_system_prompt: false,
            }));
        }

        // If no API keys are available, add default models with empty keys
        if models.is_empty() {
            models.extend_from_slice(&[
                ModelConfig::Claude(ClaudeConf {
                    name: "sonnet".to_string(),
                    api_model: ANTHROPIC_CLAUDE_SONNET.to_string(),
                    key: "".to_string(),
                    key_env: ANTHROPIC_API_KEY.to_string(),
                }),
                ModelConfig::Claude(ClaudeConf {
                    name: "haiku".to_string(),
                    api_model: ANTHROPIC_CLAUDE_HAIKU.to_string(),
                    key: "".to_string(),
                    key_env: ANTHROPIC_API_KEY.to_string(),
                }),
            ]);
        }

        Self {
            include: Include::Git,
            exclude: Vec::new(),
            models,
            session_store_dir: home_config_dir().join("state"),
            retry_limit: DEFAULT_RETRY_LIMIT,
            no_preflight: false,
            default_dialect: ConfigDialect::default(),
            dummy_model: None,
            dummy_dialect: None,
            tags: Tags::default(),
            ops: Ops::default(),
            default_context: DefaultContext::default(),
            default_model: None,
            full: false,
            checks: Checks::default(),
            formatters: Formatters::default(),
            project_root: ProjectRoot::default(),
            test_cwd: None,
            no_stream: false,
        }
    }
}

impl Config {
    pub fn cwd(&self) -> Result<PathBuf> {
        if let Some(test_cwd) = &self.test_cwd {
            Ok(PathBuf::from(test_cwd))
        } else {
            env::current_dir()
                .map_err(|e| TenxError::Internal(format!("Failed to get current directory: {}", e)))
        }
    }

    pub fn with_test_cwd(mut self, path: PathBuf) -> Self {
        self.test_cwd = Some(path.to_string_lossy().into_owned());
        self
    }

    pub fn project_root(&self) -> PathBuf {
        match &self.project_root {
            ProjectRoot::Discover => find_project_root(&self.cwd().unwrap_or_default()),
            ProjectRoot::Path(path) => path.clone(),
        }
    }

    /// Calculates the relative path from the root to the given absolute path.
    pub fn relpath(&self, path: &Path) -> PathBuf {
        diff_paths(path, self.project_root()).unwrap_or_else(|| path.to_path_buf())
    }

    /// Converts a path relative to the root directory to an absolute path
    pub fn abspath(&self, path: &Path) -> Result<PathBuf> {
        let p = self.project_root().join(path);
        absolute(p.clone())
            .map_err(|e| TenxError::Internal(format!("could not absolute {}: {}", p.display(), e)))
    }

    /// Normalizes a path specification.
    ///
    /// - If the path is a glob, it will be returned as-is.
    /// - If the path is relative (i.e. starts with ./ or ../), it will be resolved relative to the
    ///   current directory.
    /// - If the path is absolute, it will be returned as-is.
    /// - Otherwise, it will be resolved relative to the project root.
    pub fn normalize_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        self.normalize_path_with_cwd(path, self.cwd()?)
    }

    /// Normalizes a path specification.
    ///
    /// - If the path is a glob, it will be returned as-is.
    /// - If the path is relative (i.e. starts with ./ or ../), it will be resolved relative to the
    ///   current directory.
    /// - If the path is absolute, it will be returned as-is.
    /// - Otherwise, it will be resolved relative to the project root.
    pub fn normalize_path_with_cwd<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        path: P,
        current_dir: Q,
    ) -> Result<PathBuf> {
        let path = path.as_ref();
        if path.to_str().map_or(false, |s| s.contains('*')) {
            return Ok(path.to_path_buf());
        }

        let absolute_path = if is_relative(path) {
            current_dir.as_ref().join(path)
        } else if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root().join(path)
        };

        let abspath = absolute(absolute_path.clone()).map_err(|e| {
            TenxError::Internal(format!(
                "Could not absolute {}: {}",
                absolute_path.display(),
                e
            ))
        })?;
        let project_root = absolute(self.project_root())
            .map_err(|e| TenxError::Internal(format!("Could not absolute project root: {}", e)))?;
        Ok(abspath
            .strip_prefix(&project_root)
            .unwrap_or(&abspath)
            .to_path_buf()
            .normalize())
    }

    /// Traverse the included files and return a list of files that match the given glob pattern.
    pub fn match_files_with_glob(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        let project_root = &self.project_root();
        let glob = Glob::new(pattern)
            .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?;
        let included_files = self.included_files()?;

        let current_dir = self.cwd()?;

        let mut matched_files = Vec::new();

        for file in included_files {
            let relative_path = if file.is_absolute() {
                file.strip_prefix(project_root).unwrap_or(&file)
            } else {
                &file
            };

            let match_path = if current_dir != *project_root {
                // If we're in a subdirectory, we need to adjust the path for matching
                diff_paths(
                    relative_path,
                    current_dir
                        .strip_prefix(project_root)
                        .unwrap_or(Path::new("")),
                )
                .unwrap_or_else(|| relative_path.to_path_buf())
            } else {
                relative_path.to_path_buf()
            };

            if glob.compile_matcher().is_match(&match_path) {
                let absolute_path = project_root.join(relative_path);
                if absolute_path.exists() {
                    matched_files.push(relative_path.to_path_buf());
                } else {
                    return Err(TenxError::Internal(format!(
                        "File does not exist: {:?}",
                        absolute_path
                    )));
                }
            }
        }

        Ok(matched_files)
    }

    pub fn included_files(&self) -> Result<Vec<PathBuf>> {
        let project_root = self.project_root();

        // Build exclude globset
        let mut exclude_builder = GlobSetBuilder::new();
        for pattern in &self.exclude {
            exclude_builder
                .add(Glob::new(pattern).map_err(|e| TenxError::Internal(e.to_string()))?);
        }
        let exclude_globset = exclude_builder
            .build()
            .map_err(|e| TenxError::Internal(e.to_string()))?;

        // Get initial file list
        let initial_files = match &self.include {
            Include::Git => {
                let output = Command::new("git")
                    .arg("ls-files")
                    .current_dir(&project_root)
                    .output()
                    .map_err(|e| {
                        TenxError::Internal(format!("Failed to execute git ls-files: {}", e))
                    })?;

                if !output.status.success() {
                    return Err(TenxError::Internal(
                        "git ls-files command failed".to_string(),
                    ));
                }

                let files = String::from_utf8(output.stdout).map_err(|e| {
                    TenxError::Internal(format!("Failed to parse git ls-files output: {}", e))
                })?;

                files
                    .lines()
                    .map(|line| PathBuf::from(line.trim()))
                    .collect::<Vec<_>>()
            }
            Include::Glob(patterns) => {
                let mut builder = GlobSetBuilder::new();
                for pattern in patterns {
                    builder
                        .add(Glob::new(pattern).map_err(|e| TenxError::Internal(e.to_string()))?);
                }
                let globset = builder
                    .build()
                    .map_err(|e| TenxError::Internal(e.to_string()))?;

                let mut included_files = Vec::new();
                walk_directory(&project_root, &project_root, &globset, &mut included_files)?;
                included_files
            }
        };

        // Filter out excluded files
        Ok(initial_files
            .into_iter()
            .filter(|path| !exclude_globset.is_match(path))
            .collect())
    }

    /// Sets the full serialization flag.
    pub fn with_full(mut self, full: bool) -> Self {
        self.full = full;
        self
    }

    /// Deserialize a TOML string into a Config.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str)
            .map_err(|e| TenxError::Internal(format!("Failed to parse TOML: {}", e)))
    }

    /// Merge another Config into this one, only overriding non-default values.
    pub fn merge(&mut self, other: &Config) {
        let dflt = Config::default();
        if other.include != dflt.include {
            self.include = other.include.clone();
        }
        if !other.session_store_dir.as_os_str().is_empty()
            && other.session_store_dir != dflt.session_store_dir
        {
            self.session_store_dir = other.session_store_dir.clone();
        }
        if other.retry_limit != dflt.retry_limit {
            self.retry_limit = other.retry_limit;
        }
        if other.no_preflight != dflt.no_preflight {
            self.no_preflight = other.no_preflight;
        }
        if !other.models.is_empty() && other.models != dflt.models {
            self.models = other.models.clone();
        }
        if other.default_dialect != dflt.default_dialect {
            self.default_dialect = other.default_dialect.clone();
        }
        if other.tags != dflt.tags {
            self.tags = other.tags.clone();
        }
        if other.ops != dflt.ops {
            self.ops = other.ops.clone();
        }
        if other.default_context != dflt.default_context {
            self.default_context = other.default_context.clone();
        }
    }

    /// Serialize the Config into a TOML string.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self)
            .map_err(|e| TenxError::Internal(format!("Failed to serialize to TOML: {}", e)))
    }

    pub fn with_dummy_model(mut self, model: model::DummyModel) -> Self {
        self.dummy_model = Some(model);
        self
    }

    pub fn with_dummy_dialect(mut self, dialect: dialect::DummyDialect) -> Self {
        self.dummy_dialect = Some(dialect);
        self
    }

    pub fn with_root<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.project_root = ProjectRoot::Path(path.as_ref().into());
        self
    }

    /// Loads API keys from environment variables if they exist.
    pub fn load_env(mut self) -> Self {
        self.models = self.models.into_iter().map(|m| m.load_env()).collect();
        self
    }

    /// Returns the configured model.
    pub fn model(&self) -> Result<crate::model::Model> {
        if let Some(dummy_model) = &self.dummy_model {
            return Ok(model::Model::Dummy(dummy_model.clone()));
        }

        let model_config = if let Some(name) = &self.default_model {
            self.models
                .iter()
                .find(|m| m.name() == name)
                .ok_or_else(|| TenxError::Internal(format!("Model {} not found", name)))?
        } else {
            self.models
                .first()
                .ok_or_else(|| TenxError::Internal("No model configured".to_string()))?
        };

        match model_config {
            ModelConfig::Claude(conf) => Ok(model::Model::Claude(conf.to_model(self.no_stream)?)),
            ModelConfig::OpenAi(conf) => Ok(model::Model::OpenAi(conf.to_model(self.no_stream))),
        }
    }

    /// Returns the configured dialect.
    pub fn dialect(&self) -> Result<crate::dialect::Dialect> {
        if let Some(dummy_dialect) = &self.dummy_dialect {
            return Ok(dialect::Dialect::Dummy(dummy_dialect.clone()));
        }
        match self.default_dialect {
            ConfigDialect::Tags => Ok(dialect::Dialect::Tags(dialect::Tags::new(
                self.tags.smart,
                self.tags.replace,
                self.tags.udiff,
                self.ops.edit,
            ))),
        }
    }

    pub fn validators(&self) -> Vec<Box<dyn Check>> {
        builtin_validators()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils;

    use tempfile::TempDir;

    macro_rules! set_config {
        ($config:expr, $($field:ident).+, $value:expr) => {
            $config.$($field).+ = $value;
        };
    }

    #[test]
    fn test_toml_serialization() {
        let mut config = Config::default();
        if let ModelConfig::Claude(conf) = &mut config.models[0] {
            conf.key = "test_key".to_string();
        }
        set_config!(config, session_store_dir, PathBuf::from("/tmp/test"));
        set_config!(config, retry_limit, 5);
        set_config!(config, no_preflight, true);
        set_config!(config, tags.smart, false);
        set_config!(config, ops.edit, false);
        set_config!(config, default_dialect, ConfigDialect::Tags);

        let toml_str = config.to_toml().unwrap();

        let deserialized_config = Config::from_toml(&toml_str).unwrap();

        if let (ModelConfig::Claude(conf), ModelConfig::Claude(deserial_conf)) =
            (&config.models[0], &deserialized_config.models[0])
        {
            assert_eq!(conf.key, deserial_conf.key);
            assert_eq!(
                config.session_store_dir,
                deserialized_config.session_store_dir
            );
            assert_eq!(config.retry_limit, deserialized_config.retry_limit);
            assert_eq!(config.no_preflight, deserialized_config.no_preflight);
            assert_eq!(config.default_dialect, deserialized_config.default_dialect);
            assert_eq!(config.tags.smart, deserialized_config.tags.smart);
            assert_eq!(config.ops.edit, deserialized_config.ops.edit);
        }

        // Test default value serialization
        let default_config = Config::default();
        let default_toml_str = default_config.to_toml().unwrap();

        let parsed_toml: toml::Value = toml::from_str(&default_toml_str).unwrap();
        let table = parsed_toml.as_table().unwrap();

        assert!(!table.contains_key("anthropic_key"));
        assert!(!table.contains_key("session_store_dir"));
        assert!(!table.contains_key("retry_limit"));
        assert!(!table.contains_key("no_preflight"));
        assert!(!table.contains_key("default_model"));
        assert!(!table.contains_key("default_dialect"));
        assert!(!table.contains_key("tags"));
        assert!(!table.contains_key("ops"));
    }

    #[test]
    fn test_include_serialization() {
        let mut config = Config::default();
        set_config!(
            config,
            include,
            Include::Glob(vec!["*.rs".to_string(), "*.toml".to_string()])
        );

        let toml_str = config.to_toml().unwrap();

        let deserialized_config = Config::from_toml(&toml_str).unwrap();

        assert!(matches!(deserialized_config.include, Include::Glob(_)));
        if let Include::Glob(patterns) = deserialized_config.include {
            assert_eq!(patterns, vec!["*.rs".to_string(), "*.toml".to_string()]);
        }

        // Test default value (Git) is not serialized
        let default_config = Config::default();
        let default_toml_str = default_config.to_toml().unwrap();
        let parsed_toml: toml::Value = toml::from_str(&default_toml_str).unwrap();
        let table = parsed_toml.as_table().unwrap();
        assert!(!table.contains_key("include"));
    }

    #[test]
    fn test_config_merge() {
        let mut base_config = Config::default();
        if let ModelConfig::Claude(conf) = &mut base_config.models[0] {
            conf.key = "base_key".to_string();
        }
        set_config!(base_config, retry_limit, 5);

        let mut other_config = Config::default();
        if let ModelConfig::Claude(conf) = &mut other_config.models[0] {
            conf.key = "other_key".to_string();
        }
        set_config!(other_config, session_store_dir, PathBuf::from("/tmp/other"));
        set_config!(other_config, no_preflight, true);
        set_config!(
            other_config,
            include,
            Include::Glob(vec!["*.rs".to_string()])
        );

        base_config.merge(&other_config);

        if let ModelConfig::Claude(conf) = &base_config.models[0] {
            assert_eq!(conf.key, "other_key".to_string());
            assert_eq!(base_config.session_store_dir, PathBuf::from("/tmp/other"));
            assert_eq!(base_config.retry_limit, 5);
            assert!(base_config.no_preflight);
            assert_eq!(base_config.default_dialect, ConfigDialect::Tags);
            assert!(!base_config.tags.smart);
            assert!(matches!(base_config.include, Include::Glob(_)));
            if let Include::Glob(patterns) = &base_config.include {
                assert_eq!(patterns, &vec!["*.rs".to_string()]);
            }
        }
    }

    #[test]
    fn test_session_store_dir_option() {
        let config = Config::default();

        let mut config_with_dir = config.clone();
        set_config!(
            config_with_dir,
            session_store_dir,
            PathBuf::from("/tmp/test")
        );
        assert_eq!(
            config_with_dir.session_store_dir,
            PathBuf::from("/tmp/test")
        );

        let config_without_change = config.clone();
        assert_eq!(
            config_without_change.session_store_dir,
            home_config_dir().join("state")
        );

        let mut config_with_existing = Config::default();
        set_config!(
            config_with_existing,
            session_store_dir,
            PathBuf::from("/tmp/existing")
        );

        let mut config_override = config_with_existing.clone();
        set_config!(
            config_override,
            session_store_dir,
            PathBuf::from("/tmp/new")
        );
        assert_eq!(config_override.session_store_dir, PathBuf::from("/tmp/new"));

        let config_keep_existing = config_with_existing.clone();
        assert_eq!(
            config_keep_existing.session_store_dir,
            PathBuf::from("/tmp/existing")
        );
    }

    #[test]
    fn test_single_value_deserialization() {
        let toml_str = "retry_limit = 42";
        let mut config = Config::from_toml(toml_str).unwrap();
        config.models = vec![ModelConfig::Claude(ClaudeConf {
            name: "sonnet".to_string(),
            api_model: "test".to_string(),
            key: "".to_string(),
            key_env: ANTHROPIC_API_KEY.to_string(),
        })];

        assert_eq!(config.retry_limit, 42);
        if let ModelConfig::Claude(conf) = &config.models[0] {
            assert_eq!(conf.key, "");
            assert_eq!(config.session_store_dir, PathBuf::new());
            assert!(!config.no_preflight);
            let default_model = &config.models[0];
            assert!(matches!(default_model, ModelConfig::Claude(_)));
            assert_eq!(config.default_dialect, ConfigDialect::Tags);
            assert!(!config.tags.smart);
        }
    }

    #[test]
    fn test_included_files() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path();

        testutils::create_file_tree(
            root_path,
            &[
                "file1.rs",
                "file2.txt",
                "subdir/file3.rs",
                "subdir/file4.txt",
                "subdir/ignore.rs",
            ],
        )?;

        let config = Config {
            include: Include::Glob(vec!["*.rs".to_string(), "subdir/*.txt".to_string()]),
            exclude: vec!["**/ignore.rs".to_string()],
            project_root: ProjectRoot::Path(root_path.to_path_buf()),
            ..Default::default()
        };

        let mut included_files = config.included_files()?;
        included_files.sort();

        let mut expected_files = vec![
            PathBuf::from("file1.rs"),
            PathBuf::from("subdir/file3.rs"),
            PathBuf::from("subdir/file4.txt"),
        ];
        expected_files.sort();

        assert_eq!(included_files, expected_files);

        // Test with multiple exclude patterns
        let config_multi_exclude = Config {
            include: Include::Glob(vec!["**/*.rs".to_string(), "**/*.txt".to_string()]),
            exclude: vec!["**/ignore.rs".to_string(), "subdir/*.txt".to_string()],
            project_root: ProjectRoot::Path(root_path.to_path_buf()),
            ..Default::default()
        };

        let mut included_files = config_multi_exclude.included_files()?;
        included_files.sort();

        let mut expected_files = vec![
            PathBuf::from("file1.rs"),
            PathBuf::from("file2.txt"),
            PathBuf::from("subdir/file3.rs"),
        ];
        expected_files.sort();

        assert_eq!(included_files, expected_files);

        Ok(())
    }

    #[test]
    fn test_project_root() {
        let config_discover = Config::default();
        assert!(matches!(
            config_discover.project_root,
            ProjectRoot::Discover
        ));

        let config_path = Config {
            project_root: ProjectRoot::Path(PathBuf::from("/custom/path")),
            ..Default::default()
        };
        assert_eq!(config_path.project_root(), PathBuf::from("/custom/path"));
    }

    #[test]
    fn test_match_files_with_glob() -> Result<()> {
        use crate::testutils::test_project;

        let mut project = test_project();
        project.create_file_tree(&[
            "src/file1.rs",
            "src/subdir/file2.rs",
            "tests/test1.rs",
            "README.md",
        ]);

        project.config.include =
            Include::Glob(vec!["**/*.rs".to_string(), "README.md".to_string()]);

        // Test matching files from root directory
        let matched_files = project.config.match_files_with_glob("src/**/*.rs")?;
        assert_eq!(
            matched_files.len(),
            2,
            "Expected 2 matched files, got {}",
            matched_files.len()
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/file1.rs")),
            "src/file1.rs not matched"
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/subdir/file2.rs")),
            "src/subdir/file2.rs not matched"
        );

        // Test matching files from subdirectory
        project.set_cwd("src");
        let matched_files = project.config.match_files_with_glob("**/*.rs")?;
        assert_eq!(
            matched_files.len(),
            3,
            "Expected 3 matched files, got {}",
            matched_files.len()
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/file1.rs")),
            "src/file1.rs not matched"
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/subdir/file2.rs")),
            "src/subdir/file2.rs not matched"
        );
        assert!(
            matched_files.contains(&PathBuf::from("tests/test1.rs")),
            "tests/test1.rs not matched"
        );

        // Test matching non-Rust files
        let matched_files = project.config.match_files_with_glob("*.md")?;
        assert_eq!(
            matched_files.len(),
            1,
            "Expected 1 matched file, got {}",
            matched_files.len()
        );
        assert!(
            matched_files.contains(&PathBuf::from("README.md")),
            "README.md not matched"
        );

        Ok(())
    }

    #[test]
    fn test_normalize_path_with_cwd() -> Result<()> {
        let project = testutils::test_project();
        project.create_file_tree(&[
            "file.txt",
            "subdir/subfile.txt",
            "../outside/outsidefile.txt",
            "abs_file.txt",
        ]);

        let root = project.tempdir.path();
        let sub_dir = root.join("subdir");
        let outside_dir = root.parent().unwrap().join("outside");
        let cnf = project.config.clone();

        // Test 1: Current dir is the root directory
        assert_eq!(
            cnf.normalize_path_with_cwd("file.txt", root)?,
            PathBuf::from("file.txt")
        );
        assert_eq!(
            cnf.normalize_path_with_cwd("./file.txt", root)?,
            PathBuf::from("file.txt")
        );

        // Test 2: Current dir is under the root directory
        assert_eq!(
            cnf.normalize_path_with_cwd("./subfile.txt", &sub_dir)?,
            PathBuf::from("subdir/subfile.txt")
        );
        assert_eq!(
            cnf.normalize_path_with_cwd("../file.txt", &sub_dir)?,
            PathBuf::from("file.txt")
        );
        assert_eq!(
            cnf.normalize_path_with_cwd("file.txt", &sub_dir)?,
            PathBuf::from("file.txt")
        );

        // Test 3: Current dir is outside the root directory
        assert_eq!(
            cnf.normalize_path_with_cwd("file.txt", &outside_dir)?,
            PathBuf::from("file.txt")
        );
        assert_eq!(
            cnf.normalize_path_with_cwd("./outside_file.txt", &outside_dir)?,
            outside_dir.join("outside_file.txt")
        );

        // Test 4: Absolute path
        let abs_path = root.join("abs_file.txt");
        assert_eq!(
            cnf.normalize_path_with_cwd(&abs_path, root)?,
            PathBuf::from("abs_file.txt")
        );

        Ok(())
    }
}
