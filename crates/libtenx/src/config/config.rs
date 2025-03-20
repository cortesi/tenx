use std::{
    collections::HashMap,
    env, fs,
    path::{absolute, Path, PathBuf},
};

use globset::Glob;
use optional_struct::*;
use path_clean::clean;
use pathdiff::diff_paths;
use serde::{Deserialize, Serialize};

use ron;

use crate::{
    checks,
    config::default_config,
    dialect,
    error::{self, TenxError},
    model,
};
use state;

pub const HOME_CONFIG_FILE: &str = "tenx.ron";
pub const PROJECT_CONFIG_FILE: &str = ".tenx.ron";

/// The path to the user's home configuration directory for tenx.
pub(crate) fn home_config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".config")
        .join("tenx")
}

/// Deserialize a RON string into a ConfigFile.
fn parse_config_file(ron_str: &str) -> error::Result<ConfigFile> {
    let options =
        ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    options
        .from_str(ron_str)
        .map_err(|e| TenxError::Internal(format!("Failed to parse RON: {}", e)))
}

/// Loads the configuration by merging defaults, home, and local configuration files.
/// Returns the complete Config object.
fn parse_config(
    home_config: &str,
    project_config: &str,
    current_dir: &Path,
) -> error::Result<Config> {
    let default_conf = default_config(current_dir);
    let mut cnf = ConfigFile::default();

    // Load from home config file
    if !home_config.is_empty() {
        let home_config = parse_config_file(home_config)
            .map_err(|e| TenxError::Config(format!("Failed to parse home config file: {}", e)))?;
        cnf = cnf.apply(home_config);
    }

    // Load from local config file
    if !project_config.is_empty() {
        let project_config = parse_config_file(project_config)
            .map_err(|e| TenxError::Config(format!("Failed to parse local config file: {}", e)))?;
        cnf = cnf.apply(project_config);
    }
    Ok(cnf.build(default_conf))
}

/// Loads the Tenx configuration by merging defaults, home, and local configuration files. Returns
/// the complete Config object.
pub fn load_config(current_dir: &Path) -> error::Result<Config> {
    let home_config_path = home_config_dir().join(HOME_CONFIG_FILE);
    let home_config = if home_config_path.exists() {
        fs::read_to_string(&home_config_path)
            .map_err(|e| TenxError::Config(format!("Failed to read home config file: {}", e)))?
    } else {
        String::new()
    };

    let default_conf = default_config(current_dir);
    let project_root = default_conf.project_root();
    let project_config_path = project_root.join(PROJECT_CONFIG_FILE);
    let project_config = if project_config_path.exists() {
        fs::read_to_string(&project_config_path)
            .map_err(|e| TenxError::Config(format!("Failed to read local config file: {}", e)))?
    } else {
        String::new()
    };

    parse_config(&home_config, &project_config, current_dir)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
/// Match specification for a Mode over-ride.
pub enum ModeSpec {
    Name(String),
    Globs(Vec<String>),
}

/// Mode over-ride configuration.
#[optional_struct]
#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ModeConfig {
    /// The default context configuration.
    #[optional_rename(OptionalContext)]
    #[optional_wrap]
    pub context: Context,
}

/// A named block of text to include as context in model interactions.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TextContext {
    pub name: String,
    pub content: String,
}

/// Configuration for what context to include in model interactions.
#[optional_struct]
#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Defines which context is included in model interactions.
pub struct Context {
    pub ruskel: Vec<String>,
    pub path: Vec<String>,
    pub project_map: bool,
    pub text: Vec<TextContext>,
    pub cmd: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Configuration for a specific model provider (Claude, OpenAI, or Google).
pub enum Model {
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
        /// Reasoning effort for OpenAI o1 and o3 models.
        reasoning_effort: Option<ReasoningEffort>,
    },
    Google {
        /// The name of the model.
        name: String,
        /// The API model identifier.
        api_model: String,
        /// The API key.
        key: String,
        /// The environment variable to load the API key from.
        key_env: String,
        /// Whether the model can stream responses.
        can_stream: bool,
    },
}

impl Model {
    /// Loads API key from environment if key is empty and key_env is specified.
    pub fn load_env(mut self) -> Self {
        match self {
            Model::Claude {
                ref mut key,
                ref key_env,
                ..
            }
            | Model::OpenAi {
                ref mut key,
                ref key_env,
                ..
            }
            | Model::Google {
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
            Model::Claude { name, .. } => name,
            Model::OpenAi { name, .. } => name,
            Model::Google { name, .. } => name,
        }
    }

    /// Returns the kind of model (e.g. "claude").
    pub fn kind(&self) -> &'static str {
        match self {
            Model::Claude { .. } => "claude",
            Model::OpenAi { .. } => "openai",
            Model::Google { .. } => "google",
        }
    }

    /// Returns the API model identifier.
    pub fn api_model(&self) -> &str {
        match self {
            Model::Claude { api_model, .. } => api_model,
            Model::OpenAi { api_model, .. } => api_model,
            Model::Google { api_model, .. } => api_model,
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
            Model::Claude {
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
            Model::OpenAi {
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
            Model::Google {
                api_model,
                key,
                key_env,
                can_stream,
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
                    format!("stream = {}", can_stream),
                ]
                .join("\n")
            }
        }
    }

    /// Converts ModelConfig to a Claude, OpenAi, or Google model.
    pub fn to_model(&self, no_stream: bool) -> error::Result<model::Model> {
        match self {
            Model::Claude { api_model, key, .. } => {
                if api_model.is_empty() {
                    return Err(TenxError::Model("Empty API model name".into()));
                }
                if key.is_empty() {
                    return Err(TenxError::Model("Empty Anthropic API key".into()));
                }
                Ok(model::Model::Claude(model::Claude {
                    name: self.name().to_string(),
                    api_model: api_model.clone(),
                    anthropic_key: key.clone(),
                    streaming: !no_stream,
                }))
            }
            Model::OpenAi {
                api_model,
                key,
                api_base,
                can_stream,
                no_system_prompt,
                reasoning_effort,
                ..
            } => Ok(model::Model::OpenAi(model::OpenAi {
                name: self.name().to_string(),
                api_model: api_model.clone(),
                openai_key: key.clone(),
                api_base: api_base.clone(),
                streaming: *can_stream && !no_stream,
                no_system_prompt: *no_system_prompt,
                reasoning_effort: match reasoning_effort {
                    Some(ReasoningEffort::Low) => Some(model::ReasoningEffort::Low),
                    Some(ReasoningEffort::Medium) => Some(model::ReasoningEffort::Medium),
                    Some(ReasoningEffort::High) => Some(model::ReasoningEffort::High),
                    None => None,
                },
            })),
            Model::Google {
                api_model,
                key,
                can_stream,
                ..
            } => {
                if api_model.is_empty() {
                    return Err(TenxError::Model("Empty API model name".into()));
                }
                if key.is_empty() {
                    return Err(TenxError::Model("Empty Google API key".into()));
                }
                Ok(model::Model::Google(model::Google {
                    name: self.name().to_string(),
                    api_model: api_model.clone(),
                    api_key: key.clone(),
                    streaming: *can_stream && !no_stream,
                }))
            }
        }
    }
}

/// Settings related to the dialect we are using to communicate to models. For the moment, we have
/// only one dialect, so this section is pretty simple.
#[optional_struct]
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Settings related to the dialect used for model communication.
pub struct Dialect {
    /// Allow the model to request to edit files in the project map
    pub edit: bool,
}

/// Project configuration.
#[optional_struct]
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Project configuration including root directory and file inclusion rules.
pub struct Project {
    /// Project root configuration.
    pub root: PathBuf,

    /// Glob patterns for file inclusion/exclusion. Patterns prefixed with "!" exclude matches.
    /// For example: ["*.rs", "!test_*.rs"] includes all Rust files except test files. Unless
    /// over-ridden, Tenx respects .gitignore, .ignore and .git/info/exclude files.
    #[serde(default)]
    pub include: Vec<String>,
}

#[optional_struct]
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Configuration for checks.
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

#[optional_struct]
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Configuration for available models.
pub struct Models {
    /// Custom model configurations. Entries with the same name as a builtin will override the
    /// builtin.
    #[serde(default)]
    pub custom: Vec<Model>,

    /// Built-in model configurations.
    #[serde(default)]
    pub builtin: Vec<Model>,

    /// The default model name.
    #[serde(default)]
    pub default: String,

    /// Disable streaming for all models
    #[serde(default)]
    pub no_stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// When a check should run - before changes, after changes, or both.
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Configuration for a specific check.
pub struct CheckConfig {
    /// Name of the validator for display and error reporting
    pub name: String,

    /// Shell command to execute, run with sh -c
    pub command: String,

    /// List of glob patterns to match against files for determining relevance
    pub globs: Vec<String>,

    /// Whether this validator defaults to off in the configuration
    #[serde(default)]
    pub default_off: bool,

    /// Whether to treat any stderr output as a failure, regardless of exit code
    #[serde(default)]
    pub fail_on_stderr: bool,
}

impl CheckConfig {
    /// Converts a CheckConfig to a concrete Check object.
    pub fn to_check(&self) -> checks::Check {
        checks::Check {
            name: self.name.clone(),
            command: self.command.clone(),
            globs: self.globs.clone(),
            default_off: self.default_off,
            fail_on_stderr: self.fail_on_stderr,
        }
    }
}

#[optional_struct(ConfigFile)]
#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
/// Primary configuration struct containing all settings.
pub struct Config {
    /// Model configuration
    #[optional_rename(OptionalModels)]
    #[optional_wrap]
    pub models: Models,

    /// Project configuration
    #[optional_rename(OptionalProject)]
    #[optional_wrap]
    pub project: Project,

    /// The directory to store session state. Defaults to ~/.config/tenx/state
    pub session_store_dir: PathBuf,

    /// The number of steps we can take autonomously without user input. This doesn't limit the
    /// total number of steps in a session.
    pub step_limit: usize,

    /// Operations that can be executed by the model.
    #[optional_rename(OptionalDialect)]
    #[optional_wrap]
    pub dialect: Dialect,

    /// The default context configuration.
    #[optional_rename(OptionalContext)]
    #[optional_wrap]
    pub context: Context,

    /// Check configuration.
    #[optional_rename(OptionalChecks)]
    #[optional_wrap]
    pub checks: Checks,

    /// Mode configuration
    pub modes: HashMap<ModeSpec, ModeConfig>,

    // Internal fields, not to be set in config
    //
    /// Set a dummy model for end-to-end testing. Over-rides the configured model.
    #[serde(skip)]
    pub(crate) dummy_model: Option<model::DummyModel>,

    /// Set a dummy dialect for end-to-end testing. Over-rides the configured dialect.
    #[serde(skip)]
    pub(crate) dummy_dialect: Option<dialect::DummyDialect>,

    /// The current working directory when testing. We need this, because we can't change the CWD
    /// reliably in tests for reasons of concurrency.
    #[serde(skip)]
    pub(crate) cwd: Option<PathBuf>,
}

impl Config {
    /// Returns all model configurations, with custom models overriding built-in models with the same name.
    pub fn model_confs(&self) -> Vec<Model> {
        let builtin = self
            .models
            .builtin
            .iter()
            .map(|m| (m.name().to_string(), m.clone()));
        let custom = self
            .models
            .custom
            .iter()
            .map(|m| (m.name().to_string(), m.clone()));

        let mut model_map: HashMap<String, Model> = builtin.collect();
        model_map.extend(custom);

        let mut models: Vec<Model> = model_map.into_values().collect();
        models.sort_by(|a, b| a.name().cmp(b.name()));
        models
    }

    pub fn cwd(&self) -> error::Result<PathBuf> {
        if let Some(test_cwd) = &self.cwd {
            Ok(PathBuf::from(test_cwd))
        } else {
            env::current_dir()
                .map_err(|e| TenxError::Internal(format!("Failed to get current directory: {}", e)))
        }
    }

    /// Sets the current working directory, so we don't consult the environment
    pub fn with_cwd(mut self, path: PathBuf) -> Self {
        self.cwd = Some(path);
        self
    }

    pub fn project_root(&self) -> PathBuf {
        if self.project.root.to_string_lossy().is_empty() {
            ".".into()
        } else {
            self.project.root.clone()
        }
    }

    /// Calculates the relative path from the root to the given absolute path.
    pub fn relpath(&self, path: &Path) -> PathBuf {
        diff_paths(path, self.project_root()).unwrap_or_else(|| path.to_path_buf())
    }

    /// Converts a path relative to the root directory to an absolute path
    pub fn abspath(&self, path: &Path) -> error::Result<PathBuf> {
        let p = self.project_root().join(path);
        absolute(p.clone())
            .map_err(|e| TenxError::Internal(format!("could not absolute {}: {}", p.display(), e)))
    }

    /// Normalizes a path specification.
    ///
    /// Any resulting path is either a) relative to the project root, or b) an absolute path.
    ///
    /// - If the path is relative (i.e. starts with ./ or ../), it is first resolved to an absolute
    ///   path relative to the current directory, then rebased to be relative to the project root.
    /// - If the path starts with "**", it will be returned as-is.
    /// - If the path is absolute, it will be returned as-is if it is outside the project root,
    ///   otherwise it will be rebased to be relative to the project root.
    pub fn normalize_path<P: AsRef<Path>>(&self, path: P) -> error::Result<PathBuf> {
        self.normalize_path_with_cwd(path, self.cwd()?)
    }

    /// Normalizes a path specification.
    ///
    /// Any resulting path is either a) relative to the project root, or b) an absolute path.
    ///
    /// - If the path is relative (i.e. starts with ./ or ../), it is first resolved to an absolute
    ///   path relative to the current directory, then rebased to be relative to the project root.
    /// - If the path starts with "**", it will be returned as-is.
    /// - If the path is absolute, it will be returned as-is if it is outside the project root,
    ///   otherwise it will be rebased to be relative to the project root.
    pub fn normalize_path_with_cwd<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        path: P,
        current_dir: Q,
    ) -> error::Result<PathBuf> {
        let path = path.as_ref();
        let current_dir = current_dir.as_ref();

        let project_root = absolute(self.project_root())
            .map_err(|e| TenxError::Internal(format!("Could not absolute project root: {}", e)))?;

        Ok(clean(if path.is_absolute() {
            diff_paths(path, &project_root).unwrap_or(path.to_path_buf())
        } else if path.starts_with("**") {
            path.to_path_buf()
        } else {
            let abs_path = absolute(current_dir.join(path))
                .map_err(|e| TenxError::Internal(format!("Could not absolute path: {}", e)))?;
            diff_paths(&abs_path, &project_root).unwrap_or(path.to_path_buf())
        }))
    }

    /// Traverse the included files and return a list of files that match the given glob pattern.
    pub fn match_files_with_glob(&self, pattern: &str) -> error::Result<Vec<PathBuf>> {
        let project_root = &self.project_root();
        let glob = Glob::new(pattern)
            .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?;
        let included_files = self.project_files()?;

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

    /// Construct the default state for the project, including the project root directory, and
    /// a memory overlay for files prefixed with "::".
    pub fn state(&self) -> error::Result<state::State> {
        let s = state::State::default()
            .with_directory(&self.project.root, self.project.include.clone())?;
        Ok(s)
    }

    pub fn project_files(&self) -> error::Result<Vec<PathBuf>> {
        let root = state::abspath::AbsPath::new(self.project.root.clone())?;
        let ret = state::files::list_files(root, self.project.include.clone())?;
        Ok(ret)
    }

    /// Serialize the Config into a RON string.
    pub fn to_ron(&self) -> error::Result<String> {
        let pretty_config = ron::ser::PrettyConfig::default();
        ron::ser::to_string_pretty(self, pretty_config)
            .map_err(|e| TenxError::Internal(format!("Failed to serialize to RON: {}", e)))
    }

    pub fn with_dummy_model(mut self, model: model::DummyModel) -> Self {
        self.dummy_model = Some(model);
        self
    }

    pub fn with_root<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.project.root = path.as_ref().into();
        self
    }

    /// Returns the model configuration for the given model name, or None if not found.
    pub fn get_model_conf<S: AsRef<str>>(&self, name: S) -> Option<Model> {
        self.model_confs()
            .into_iter()
            .find(|m| m.name() == name.as_ref())
    }

    /// Loads API keys from environment variables if they exist.
    pub fn load_env(mut self) -> Self {
        self.models.custom = self
            .models
            .custom
            .iter()
            .map(|m| m.clone().load_env())
            .collect();
        self.models.builtin = self
            .models
            .builtin
            .iter()
            .map(|m| m.clone().load_env())
            .collect();
        self
    }

    /// Returns the configured model.
    pub fn active_model(&self) -> error::Result<model::Model> {
        if let Some(dummy_model) = &self.dummy_model {
            return Ok(model::Model::Dummy(dummy_model.clone()));
        }

        let name = self.models.default.clone();

        let model_config = self
            .model_confs()
            .into_iter()
            .find(|m| m.name() == name)
            .ok_or_else(|| TenxError::Internal(format!("Model {} not found", name)))?;

        match model_config {
            Model::Claude {
                name,
                api_model,
                key,
                ..
            } => Ok(model::Model::Claude(model::Claude {
                name: name.clone(),
                api_model: api_model.clone(),
                anthropic_key: key.clone(),
                streaming: !self.models.no_stream,
            })),
            Model::OpenAi {
                api_model,
                key,
                api_base,
                can_stream,
                no_system_prompt,
                ..
            } => Ok(model::Model::OpenAi(model::OpenAi {
                name: name.clone(),
                api_model: api_model.clone(),
                openai_key: key.clone(),
                api_base: api_base.clone(),
                streaming: can_stream && !self.models.no_stream,
                no_system_prompt,
                reasoning_effort: None,
            })),
            Model::Google {
                name,
                api_model,
                key,
                can_stream,
                ..
            } => Ok(model::Model::Google(model::Google {
                name: name.clone(),
                api_model: api_model.clone(),
                api_key: key.clone(),
                streaming: can_stream && !self.models.no_stream,
            })),
        }
    }

    /// Returns the configured dialect.
    pub fn dialect(&self) -> error::Result<dialect::Dialect> {
        if let Some(dummy_dialect) = &self.dummy_dialect {
            return Ok(dialect::Dialect::Dummy(dummy_dialect.clone()));
        }
        Ok(dialect::Dialect::Tags(dialect::Tags::new()))
    }

    /// Return all configured checks, even if disabled. Custom checks with the same name as builtin
    /// checks replace the builtin checks. Order is preserved, with custom checks appearing in their
    /// original position if they override a builtin check.
    pub fn all_checks(&self) -> Vec<checks::Check> {
        let custom_map: HashMap<_, _> = self
            .checks
            .custom
            .iter()
            .map(|c| (c.name.clone(), c))
            .collect();

        let mut checks = Vec::new();

        // First add all builtin checks, replacing with custom if they exist
        for check in &self.checks.builtin {
            if let Some(custom) = custom_map.get(&check.name) {
                checks.push(custom.to_check());
            } else {
                checks.push(check.to_check());
            }
        }

        // Then add any remaining custom checks that didn't override builtins
        for check in &self.checks.custom {
            if !self.checks.builtin.iter().any(|b| b.name == check.name) {
                checks.push(check.to_check());
            }
        }

        checks
    }

    /// Get a check by name
    pub fn get_check<S: AsRef<str>>(&self, name: S) -> Option<checks::Check> {
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
    pub fn enabled_checks(&self) -> Vec<checks::Check> {
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

    use crate::testutils::{self, test_project};

    use tempfile::TempDir;

    #[test]
    fn test_config_merge() -> error::Result<()> {
        let project = testutils::test_project();
        let parsed = parse_config(
            r#"(models: (default: "foo", no_stream: true))"#,
            r#"(models: (default: "bar"), project: ( root: "/foo"))"#,
            &project.config.cwd()?,
        )?;
        assert_eq!(parsed.models.default, "bar");
        assert!(parsed.models.no_stream);
        assert_eq!(parsed.project.root, PathBuf::from("/foo"));
        Ok(())
    }

    #[test]
    fn test_config_roundtrip() -> error::Result<()> {
        let project = testutils::test_project();
        let mut config = default_config(&project.config.cwd()?);
        config.step_limit = 42;
        config.project.include.push("!*.test".to_string());

        let ron = config.to_ron()?;
        let current_dir = std::env::current_dir()?;
        let parsed = parse_config("", &ron, &current_dir)?;

        assert_eq!(parsed, config);
        Ok(())
    }

    #[test]
    fn test_parse_config_value() -> error::Result<()> {
        // Test loading a config with a custom step_limit
        let project = testutils::test_project();
        let test_config = r#"(step_limit: 10)"#;
        let config = parse_config("", test_config, &project.config.cwd()?)?;
        assert_eq!(config.step_limit, 10);

        // Test that other values remain at default
        let project = testutils::test_project();
        let default_config = default_config(&project.config.cwd()?);
        assert_eq!(config.models, default_config.models);
        assert_eq!(config.project.include, default_config.project.include);

        Ok(())
    }

    macro_rules! set_config {
        ($config:expr, $($field:ident).+, $value:expr) => {
            $config.$($field).+ = $value;
        };
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
    fn test_included_files() -> error::Result<()> {
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
            project: Project {
                root: root_path.to_path_buf(),
                include: vec![
                    "**/*.rs".to_string(),
                    "subdir/*.txt".to_string(),
                    "!**/ignore.rs".to_string(),
                ],
            },
            ..Default::default()
        };

        let mut included_files = config.project_files()?;
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
            project: Project {
                root: root_path.to_path_buf(),
                include: vec![
                    "**/*.rs".to_string(),
                    "**/*.txt".to_string(),
                    "!**/ignore.rs".to_string(),
                    "!subdir/*.txt".to_string(),
                ],
            },
            ..Default::default()
        };

        let mut included_files = config_multi_exclude.project_files()?;
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
        let config_path = Config {
            project: Project {
                root: PathBuf::from("/custom/path"),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(config_path.project_root(), PathBuf::from("/custom/path"));
    }

    #[test]
    fn test_match_files_with_glob() -> error::Result<()> {
        let mut project = test_project();
        project.create_file_tree(&[
            "src/file1.rs",
            "src/subdir/file2.rs",
            "tests/test1.rs",
            "README.md",
        ]);

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
    fn test_normalize_path_with_cwd() -> error::Result<()> {
        struct TestCase {
            cwd: &'static str,
            input: &'static str,
            expected: &'static str,
        }

        let tests = vec![
            // Root directory tests
            TestCase {
                cwd: "",
                input: "file.txt",
                expected: "file.txt",
            },
            TestCase {
                cwd: "",
                input: "./file.txt",
                expected: "file.txt",
            },
            // Subdirectory tests
            TestCase {
                cwd: "subdir",
                input: "./subfile.txt",
                expected: "subdir/subfile.txt",
            },
            TestCase {
                cwd: "subdir",
                input: "../file.txt",
                expected: "file.txt",
            },
            TestCase {
                cwd: "subdir",
                input: "file.txt",
                expected: "subdir/file.txt",
            },
            // Outside directory test
            TestCase {
                cwd: "../outside",
                input: "file.txt",
                expected: "../outside/file.txt",
            },
        ];

        let project = testutils::test_project();
        let root = project.tempdir.path();
        let cnf = project.config.clone();

        for test in tests {
            let cwd = if test.cwd.is_empty() {
                root.to_path_buf()
            } else if test.cwd.starts_with("..") {
                root.parent()
                    .unwrap()
                    .join(test.cwd.trim_start_matches("../"))
            } else {
                root.join(test.cwd)
            };

            let result = cnf.normalize_path_with_cwd(test.input, cwd)?;
            assert_eq!(
                result,
                PathBuf::from(test.expected),
                "Failed for cwd: {}, input: {}",
                test.cwd,
                test.input
            );
        }

        // Test absolute path separately since it requires dynamic path construction
        let abs_path = root.join("abs_file.txt");
        assert_eq!(
            cnf.normalize_path_with_cwd(&abs_path, root)?,
            PathBuf::from("abs_file.txt")
        );

        Ok(())
    }
}
