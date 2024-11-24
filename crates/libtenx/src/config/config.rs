use std::{
    collections::HashMap,
    env, fs,
    path::{absolute, Path, PathBuf},
    process::Command,
};

use globset::{Glob, GlobSetBuilder};
use normalize_path::NormalizePath;
use pathdiff::diff_paths;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use ron;

use crate::{checks::Check, dialect, model, Result, TenxError};

pub const HOME_CONFIG_FILE: &str = "tenx.ron";
pub const LOCAL_CONFIG_FILE: &str = ".tenx.ron";

macro_rules! serialize_if_different {
    ($state:expr, $full:expr, $self:expr, $default:expr, $field:ident) => {
        if $full || $self.$field != $default.$field {
            $state.serialize_field(stringify!($field), &$self.$field)?;
        }
    };
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

/// Loads the configuration by merging defaults, home, and local configuration files.
/// Returns the complete Config object.
pub fn load_config() -> Result<Config> {
    let mut config = super::default_config();

    // Load from home config file
    let home_config_path = home_config_dir().join(HOME_CONFIG_FILE);
    if home_config_path.exists() {
        let home_config_str = fs::read_to_string(&home_config_path)
            .map_err(|e| TenxError::Config(format!("Failed to read home config file: {}", e)))?;
        let home_config = Config::from_ron(&home_config_str)
            .map_err(|e| TenxError::Config(format!("Failed to parse home config file: {}", e)))?;
        config.merge(&home_config);
    }

    // Load from local config file
    let project_root = config.project_root();
    let local_config_path = project_root.join(LOCAL_CONFIG_FILE);
    if local_config_path.exists() {
        let local_config_str = fs::read_to_string(&local_config_path)
            .map_err(|e| TenxError::Config(format!("Failed to read local config file: {}", e)))?;
        let local_config = Config::from_ron(&local_config_str)
            .map_err(|e| TenxError::Config(format!("Failed to parse local config file: {}", e)))?;
        config.merge(&local_config);
    }
    Ok(config)
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelConfig {
    Claude {
        /// The name of the model.
        name: String,
        /// The API model identifier.
        api_model: String,
        /// The API key.
        key: String,
        /// The environment variable to load the API key from.
        key_env: String,
    },
    OpenAi {
        /// The name of the model.
        name: String,
        /// The API model identifier.
        api_model: String,
        /// The API key.
        key: String,
        /// The environment variable to load the API key from.
        key_env: String,
        /// The base URL for the API.
        api_base: String,
        /// Whether the model can stream responses.
        can_stream: bool,
        /// Whether the model supports a separate system prompt.
        no_system_prompt: bool,
    },
}

impl ModelConfig {
    /// Loads API key from environment if key is empty and key_env is specified.
    pub fn load_env(mut self) -> Self {
        match self {
            ModelConfig::Claude {
                ref mut key,
                ref key_env,
                ..
            }
            | ModelConfig::OpenAi {
                ref mut key,
                ref key_env,
                ..
            } => {
                if key.is_empty() && !key_env.is_empty() {
                    if let Ok(env_key) = env::var(key_env) {
                        *key = env_key;
                    }
                }
                self
            }
        }
    }

    /// Returns the name of the configured model.
    pub fn name(&self) -> &str {
        match self {
            ModelConfig::Claude { name, .. } => name,
            ModelConfig::OpenAi { name, .. } => name,
        }
    }

    /// Returns the kind of model (e.g. "claude").
    pub fn kind(&self) -> &'static str {
        match self {
            ModelConfig::Claude { .. } => "claude",
            ModelConfig::OpenAi { .. } => "openai",
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
            ModelConfig::Claude {
                api_model,
                key,
                key_env,
                ..
            } => {
                let key = if verbose {
                    key.clone()
                } else {
                    Self::abbreviate_key(key)
                };
                [
                    format!("api_model = {}", api_model),
                    format!("key = {}", key),
                    format!("key_env = {}", key_env),
                ]
                .join("\n")
            }
            ModelConfig::OpenAi {
                api_base,
                api_model,
                key,
                key_env,
                no_system_prompt,
                can_stream,
                ..
            } => {
                let key = if verbose {
                    key.clone()
                } else {
                    Self::abbreviate_key(key)
                };
                [
                    format!("api_base = {}", api_base),
                    format!("api_model = {}", api_model),
                    format!("key = {}", key),
                    format!("key_env = {}", key_env),
                    format!("no_system_prompt = {}", no_system_prompt),
                    format!("stream = {}", can_stream),
                ]
                .join("\n")
            }
        }
    }

    /// Converts ModelConfig to a Claude or OpenAi model.
    pub fn to_model(&self, no_stream: bool) -> Result<model::Model> {
        match self {
            ModelConfig::Claude { api_model, key, .. } => {
                if api_model.is_empty() {
                    return Err(TenxError::Model("Empty API model name".into()));
                }
                if key.is_empty() {
                    return Err(TenxError::Model("Empty Anthropic API key".into()));
                }
                Ok(model::Model::Claude(model::Claude {
                    api_model: api_model.clone(),
                    anthropic_key: key.clone(),
                    streaming: !no_stream,
                }))
            }
            ModelConfig::OpenAi {
                api_model,
                key,
                api_base,
                can_stream,
                no_system_prompt,
                ..
            } => Ok(model::Model::OpenAi(model::OpenAi {
                api_model: api_model.clone(),
                openai_key: key.clone(),
                api_base: api_base.clone(),
                streaming: *can_stream && !no_stream,
                no_system_prompt: *no_system_prompt,
            })),
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

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checks {
    #[serde(default)]
    pub custom: Vec<CheckConfig>,
    #[serde(default)]
    pub builtin: Vec<CheckConfig>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub no_pre: bool,
    #[serde(default)]
    pub only: Option<String>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Models {
    #[serde(default)]
    pub custom: Option<Vec<ModelConfig>>,
    #[serde(default)]
    pub builtin: Option<Vec<ModelConfig>>,
    #[serde(default)]
    pub default: Option<String>,
}

macro_rules! merge_atom {
    ($a:expr, $b:expr, $field:ident) => {
        $a.$field = match (&$a.$field, &$b.$field) {
            (Some(_), Some(rhs)) => Some(rhs.clone()),
            (None, Some(rhs)) => Some(rhs.clone()),
            (Some(lhs), None) => Some(lhs.clone()),
            (None, None) => None,
        };
    };
}

impl Models {
    pub fn merge(&mut self, other: Option<Models>) {
        if let Some(other) = other {
            merge_atom!(self, other, custom);
            merge_atom!(self, other, builtin);
            merge_atom!(self, other, default);
        }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ModeConfig {
    Pre,
    Post,
    #[default]
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckConfig {
    /// Name of the validator for display and error reporting
    pub name: String,
    /// Shell command to execute, run with sh -c
    pub command: String,
    /// List of glob patterns to match against files for determining relevance
    pub globs: Vec<String>,
    /// Whether this validator defaults to off in the configuration
    pub default_off: bool,
    /// Whether to treat any stderr output as a failure, regardless of exit code
    pub fail_on_stderr: bool,
    /// When this check should run
    pub mode: ModeConfig,
}

impl CheckConfig {
    /// Converts a CheckConfig to a concrete Check object.
    pub fn to_check(&self) -> Check {
        Check {
            name: self.name.clone(),
            command: self.command.clone(),
            globs: self.globs.clone(),
            default_off: self.default_off,
            fail_on_stderr: self.fail_on_stderr,
            mode: match self.mode {
                ModeConfig::Pre => crate::Mode::Pre,
                ModeConfig::Post => crate::Mode::Post,
                ModeConfig::Both => crate::Mode::Both,
            },
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Config {
    /// Model configuration
    #[serde(default)]
    pub models: Option<Models>,

    /// Available model configurations
    #[serde(default)]
    pub model_confs: Vec<ModelConfig>,

    /// Disable streaming for all models
    #[serde(default)]
    pub no_stream: bool,

    /// The default dialect.
    #[serde(default)]
    pub default_dialect: ConfigDialect,

    /// Which files are included by default
    #[serde(default)]
    pub include: Include,

    /// Glob patterns to exclude from the file list
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// The directory to store session state.
    #[serde(default)]
    /// The directory to store session state. Defaults to ~/.config/tenx/state
    pub session_store_dir: PathBuf,

    /// The number of times to retry a request.
    #[serde(default)]
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

    /// Project root configuration.
    #[serde(default)]
    pub project_root: ProjectRoot,

    /// Set a dummy model for end-to-end testing. Over-rides the configured model.
    #[serde(skip)]
    pub(crate) dummy_model: Option<model::DummyModel>,

    /// Set a dummy dialect for end-to-end testing. Over-rides the configured dialect.
    #[serde(skip)]
    pub(crate) dummy_dialect: Option<dialect::DummyDialect>,

    /// When true, serializes all fields regardless of default values.
    #[serde(skip)]
    pub(crate) full: bool,

    /// The current working directory when testing. We need this, because we can't change the CWD
    /// reliably in tests for reasons of concurrency.
    #[serde(skip)]
    pub(crate) test_cwd: Option<String>,
}

impl Serialize for Config {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let default = Config::default();
        let mut state = serializer.serialize_struct("Config", 11)?;
        serialize_if_different!(state, self.full, self, default, include);
        serialize_if_different!(state, self.full, self, default, model_confs);
        serialize_if_different!(state, self.full, self, default, session_store_dir);
        serialize_if_different!(state, self.full, self, default, retry_limit);
        serialize_if_different!(state, self.full, self.checks, default.checks, no_pre);
        serialize_if_different!(state, self.full, self, default, default_dialect);
        serialize_if_different!(state, self.full, self, default, tags);
        serialize_if_different!(state, self.full, self, default, ops);
        serialize_if_different!(state, self.full, self, default, default_context);
        serialize_if_different!(state, self.full, self, default, checks);
        serialize_if_different!(state, self.full, self, default, project_root);
        serialize_if_different!(state, self.full, self, default, models);
        state.end()
    }
}

impl Config {
    /// Returns all model configurations, with custom models overriding built-in models with the same name.
    pub fn model_confs(&self) -> Vec<ModelConfig> {
        if let Some(models) = &self.models {
            let builtin = models
                .builtin
                .iter()
                .flatten()
                .map(|m| (m.name().to_string(), m.clone()));
            let custom = models
                .custom
                .iter()
                .flatten()
                .map(|m| (m.name().to_string(), m.clone()));

            let mut model_map: HashMap<String, ModelConfig> = builtin.collect();
            model_map.extend(custom);

            model_map.into_values().collect()
        } else {
            Vec::new()
        }
    }

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

    /// Deserialize a RON string into a Config.
    pub fn from_ron(ron_str: &str) -> Result<Self> {
        let options = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        options
            .from_str(ron_str)
            .map_err(|e| TenxError::Internal(format!("Failed to parse RON: {}", e)))
    }

    /// Serialize the Config into a RON string.
    pub fn to_ron(&self) -> Result<String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| TenxError::Internal(format!("Failed to serialize to RON: {}", e)))
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
        if other.checks.no_pre != dflt.checks.no_pre {
            self.checks.no_pre = other.checks.no_pre;
        }
        if !other.model_confs.is_empty() && other.model_confs != dflt.model_confs {
            self.model_confs = other.model_confs.clone();
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
        if other.default_model != dflt.default_model {
            self.default_model = other.default_model.clone();
        }
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
        self.model_confs = self.model_confs.into_iter().map(|m| m.load_env()).collect();
        if let Some(models) = &mut self.models {
            if let Some(custom) = &models.custom {
                models.custom = Some(custom.iter().map(|m| m.clone().load_env()).collect());
            }
            if let Some(builtin) = &models.builtin {
                models.builtin = Some(builtin.iter().map(|m| m.clone().load_env()).collect());
            }
        }
        self
    }

    /// Returns the configured model.
    pub fn active_model(&self) -> Result<crate::model::Model> {
        if let Some(dummy_model) = &self.dummy_model {
            return Ok(model::Model::Dummy(dummy_model.clone()));
        }

        let models = self
            .models
            .as_ref()
            .ok_or_else(|| TenxError::Internal("No models configured".to_string()))?;
        let name = models
            .default
            .as_deref()
            .ok_or_else(|| TenxError::Internal("No default model specified".to_string()))?;

        let model_config = self
            .model_confs()
            .into_iter()
            .find(|m| m.name() == name)
            .ok_or_else(|| TenxError::Internal(format!("Model {} not found", name)))?;

        match model_config {
            ModelConfig::Claude { api_model, key, .. } => Ok(model::Model::Claude(model::Claude {
                api_model: api_model.clone(),
                anthropic_key: key.clone(),
                streaming: !self.no_stream,
            })),
            ModelConfig::OpenAi {
                api_model,
                key,
                api_base,
                can_stream,
                no_system_prompt,
                ..
            } => Ok(model::Model::OpenAi(model::OpenAi {
                api_model: api_model.clone(),
                openai_key: key.clone(),
                api_base: api_base.clone(),
                streaming: can_stream && !self.no_stream,
                no_system_prompt,
            })),
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

    /// Return all configured checks, even if disabled. Custom checks with the same name as builtin
    /// checks replace the builtin checks.
    pub fn all_checks(&self) -> Vec<Check> {
        let builtin = self
            .checks
            .builtin
            .iter()
            .map(|c| (c.name.clone(), c.to_check()));
        let custom = self
            .checks
            .custom
            .iter()
            .map(|c| (c.name.clone(), c.to_check()));

        let mut check_map: HashMap<String, Check> = builtin.collect();
        check_map.extend(custom);

        check_map.into_values().collect()
    }

    /// Get a check by name
    pub fn get_check<S: AsRef<str>>(&self, name: S) -> Option<Check> {
        self.all_checks()
            .into_iter()
            .find(|c| c.name == name.as_ref())
    }

    /// Returns true if a check is enabled based on its name and default state in the config
    pub fn is_check_enabled<S: AsRef<str>>(&self, name: S) -> bool {
        let name = name.as_ref();
        if let Some(check) = self.get_check(name) {
            if check.default_off() {
                // Return only if explicitly enabled
                self.checks.enable.contains(&name.to_string())
            } else {
                // Return unless explicitly disabled
                !self.checks.disable.contains(&name.to_string())
            }
        } else {
            false
        }
    }

    /// Return all enabled checks.
    pub fn enabled_checks(&self) -> Vec<Check> {
        if let Some(only_check) = &self.checks.only {
            self.all_checks()
                .into_iter()
                .filter(|check| check.name == *only_check)
                .collect()
        } else {
            self.all_checks()
                .into_iter()
                .filter(|check| self.is_check_enabled(&check.name))
                .collect()
        }
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
    fn test_ron_serialization() {
        let mut config = Config {
            model_confs: vec![ModelConfig::Claude {
                name: "sonnet".to_string(),
                api_model: "test".to_string(),
                key: "".to_string(),
                key_env: "key_env".to_string(),
            }],
            ..Default::default()
        };
        if let ModelConfig::Claude { ref mut key, .. } = &mut config.model_confs[0] {
            *key = "test_key".to_string();
        }
        set_config!(config, session_store_dir, PathBuf::from("/tmp/test"));
        set_config!(config, retry_limit, 5);
        set_config!(config.checks, no_pre, true);
        set_config!(config, tags.smart, false);
        set_config!(config, ops.edit, false);
        set_config!(config, default_dialect, ConfigDialect::Tags);

        let ron_str = config.to_ron().unwrap();

        let deserialized_config = Config::from_ron(&ron_str).unwrap();

        if let (
            ModelConfig::Claude { key: conf_key, .. },
            ModelConfig::Claude {
                key: deserial_key, ..
            },
        ) = (&config.model_confs[0], &deserialized_config.model_confs[0])
        {
            assert_eq!(conf_key, deserial_key);
            assert_eq!(
                config.session_store_dir,
                deserialized_config.session_store_dir
            );
            assert_eq!(config.retry_limit, deserialized_config.retry_limit);
            assert_eq!(config.checks.no_pre, deserialized_config.checks.no_pre);
            assert_eq!(config.default_dialect, deserialized_config.default_dialect);
            assert_eq!(config.tags.smart, deserialized_config.tags.smart);
            assert_eq!(config.ops.edit, deserialized_config.ops.edit);
        }

        // Test default value serialization
        let default_config = Config::default();
        let default_ron_str = default_config.to_ron().unwrap();

        let parsed_ron: ron::Value = ron::from_str(&default_ron_str).unwrap();
        let struct_fields_str = format!("{:?}", parsed_ron);

        assert!(!struct_fields_str.contains("anthropic_key"));
        assert!(!struct_fields_str.contains("session_store_dir"));
        assert!(!struct_fields_str.contains("retry_limit"));
        assert!(!struct_fields_str.contains("no_pre_check"));
        assert!(!struct_fields_str.contains("default_model"));
        assert!(!struct_fields_str.contains("default_dialect"));
        assert!(!struct_fields_str.contains("tags"));
        assert!(!struct_fields_str.contains("ops"));
    }

    #[test]
    fn test_include_serialization() {
        let mut config = Config::default();
        set_config!(
            config,
            include,
            Include::Glob(vec!["*.rs".to_string(), "*.toml".to_string()])
        );

        let ron_str = config.to_ron().unwrap();

        let deserialized_config = Config::from_ron(&ron_str).unwrap();

        assert!(matches!(deserialized_config.include, Include::Glob(_)));
        if let Include::Glob(patterns) = deserialized_config.include {
            assert_eq!(patterns, vec!["*.rs".to_string(), "*.toml".to_string()]);
        }

        // Test default value (Git) is not serialized
        let default_config = Config::default();
        let default_ron_str = default_config.to_ron().unwrap();
        let parsed_ron: ron::Value = ron::from_str(&default_ron_str).unwrap();
        let struct_fields_str = format!("{:?}", parsed_ron);
        assert!(!struct_fields_str.contains("include"));
    }

    #[test]
    fn test_config_merge() {
        let mut base_config = Config {
            model_confs: vec![ModelConfig::Claude {
                name: "sonnet".to_string(),
                api_model: "test".to_string(),
                key: "".to_string(),
                key_env: "key_env".to_string(),
            }],
            ..Default::default()
        };
        if let ModelConfig::Claude { ref mut key, .. } = &mut base_config.model_confs[0] {
            *key = "base_key".to_string();
        }
        set_config!(base_config, retry_limit, 5);

        let mut other_config = Config {
            model_confs: vec![ModelConfig::Claude {
                name: "sonnet".to_string(),
                api_model: "test".to_string(),
                key: "".to_string(),
                key_env: "key_env".to_string(),
            }],
            ..Default::default()
        };
        if let ModelConfig::Claude { ref mut key, .. } = &mut other_config.model_confs[0] {
            *key = "other_key".to_string();
        }
        set_config!(other_config, session_store_dir, PathBuf::from("/tmp/other"));
        set_config!(other_config.checks, no_pre, true);
        set_config!(
            other_config,
            include,
            Include::Glob(vec!["*.rs".to_string()])
        );

        base_config.merge(&other_config);

        if let ModelConfig::Claude { key, .. } = &base_config.model_confs[0] {
            assert_eq!(key, "other_key");
            assert_eq!(base_config.session_store_dir, PathBuf::from("/tmp/other"));
            assert_eq!(base_config.retry_limit, 5);
            assert!(base_config.checks.no_pre);
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
        assert_eq!(config_without_change.session_store_dir, PathBuf::new());

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
        let ron_str = "(retry_limit: 42)";
        let mut config = Config::from_ron(ron_str).unwrap();
        config.model_confs = vec![ModelConfig::Claude {
            name: "sonnet".to_string(),
            api_model: "test".to_string(),
            key: "".to_string(),
            key_env: "key_env".to_string(),
        }];

        assert_eq!(config.retry_limit, 42);
        if let ModelConfig::Claude { key, .. } = &config.model_confs[0] {
            assert_eq!(key, "");
            assert_eq!(config.session_store_dir, PathBuf::new());
            assert!(!config.checks.no_pre);
            let default_model = &config.model_confs[0];
            assert!(matches!(default_model, ModelConfig::Claude { .. }));
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

    #[test]
    fn test_models_merge() {
        // Test case 1: Merging with None
        let mut models = Models {
            custom: Some(vec![ModelConfig::Claude {
                name: "test".to_string(),
                api_model: "model1".to_string(),
                key: "key1".to_string(),
                key_env: "env1".to_string(),
            }]),
            builtin: None,
            default: Some("test".to_string()),
        };
        models.merge(None);
        assert_eq!(models.custom.as_ref().unwrap().len(), 1);
        assert_eq!(models.default.as_ref().unwrap(), "test");
        assert!(models.builtin.is_none());

        // Test case 2: Merging Some into None fields
        let mut models = Models {
            custom: None,
            builtin: None,
            default: None,
        };
        models.merge(Some(Models {
            custom: Some(vec![ModelConfig::Claude {
                name: "test".to_string(),
                api_model: "model1".to_string(),
                key: "key1".to_string(),
                key_env: "env1".to_string(),
            }]),
            builtin: Some(vec![]),
            default: Some("test".to_string()),
        }));
        assert!(models.custom.is_some());
        assert!(models.builtin.is_some());
        assert_eq!(models.default.as_ref().unwrap(), "test");

        // Test case 3: Merging Some into existing Some
        let mut models = Models {
            custom: Some(vec![ModelConfig::Claude {
                name: "test1".to_string(),
                api_model: "model1".to_string(),
                key: "key1".to_string(),
                key_env: "env1".to_string(),
            }]),
            builtin: None,
            default: Some("test1".to_string()),
        };
        models.merge(Some(Models {
            custom: Some(vec![ModelConfig::Claude {
                name: "test2".to_string(),
                api_model: "model2".to_string(),
                key: "key2".to_string(),
                key_env: "env2".to_string(),
            }]),
            builtin: None,
            default: Some("test2".to_string()),
        }));
        assert_eq!(models.custom.as_ref().unwrap()[0].name(), "test2");
        assert_eq!(models.default.as_ref().unwrap(), "test2");
    }
}
