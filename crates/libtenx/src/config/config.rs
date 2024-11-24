use std::{
    collections::HashMap,
    env, fs,
    path::{absolute, Path, PathBuf},
    process::Command,
};

use globset::{Glob, GlobSetBuilder};
use normalize_path::NormalizePath;
use optional_struct::*;
use pathdiff::diff_paths;
use serde::{Deserialize, Serialize};

use ron;

use crate::{checks::Check, config::default_config, dialect, model, TenxError};

pub const HOME_CONFIG_FILE: &str = "tenx.ron";
pub const PROJECT_CONFIG_FILE: &str = ".tenx.ron";

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
) -> crate::Result<()> {
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
        if dir.join(".git").is_dir() || dir.join(PROJECT_CONFIG_FILE).is_file() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    current_dir.to_path_buf()
}

/// Deserialize a RON string into a ConfigFile.
pub fn parse_config_file(ron_str: &str) -> crate::Result<ConfigFile> {
    let options =
        ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    options
        .from_str(ron_str)
        .map_err(|e| TenxError::Internal(format!("Failed to parse RON: {}", e)))
}

/// Loads the configuration by merging defaults, home, and local configuration files.
/// Returns the complete Config object.
pub fn parse_config(home_config: &str, project_config: &str) -> crate::Result<Config> {
    let default_conf = default_config();
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

/// Loads the configuration by merging defaults, home, and local configuration files.
/// Returns the complete Config object.
pub fn load_config() -> crate::Result<Config> {
    let home_config_path = home_config_dir().join(HOME_CONFIG_FILE);
    let home_config = if home_config_path.exists() {
        fs::read_to_string(&home_config_path)
            .map_err(|e| TenxError::Config(format!("Failed to read home config file: {}", e)))?
    } else {
        String::new()
    };

    let default_conf = default_config();
    let project_root = default_conf.project_root();
    let project_config_path = project_root.join(PROJECT_CONFIG_FILE);
    let project_config = if project_config_path.exists() {
        fs::read_to_string(&project_config_path)
            .map_err(|e| TenxError::Config(format!("Failed to read local config file: {}", e)))?
    } else {
        String::new()
    };

    parse_config(&home_config, &project_config)
}

#[optional_struct]
#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct DefaultContext {
    pub ruskel: Vec<String>,
    pub path: Vec<String>,
    pub project_map: bool,
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
    pub fn to_model(&self, no_stream: bool) -> crate::Result<model::Model> {
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

#[optional_struct]
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ops {
    /// Allow the model to request to edit files in the project map
    pub edit: bool,
}

#[optional_struct]
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tags {
    /// EXPERIMENTAL: enable smart change type
    pub smart: bool,
    /// Enable replace change type
    pub replace: bool,
    /// EXPERIMENTAL: enable udiff change type
    pub udiff: bool,
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
                write!(f, "globs:")?;
                for pattern in patterns {
                    write!(f, " {}", pattern)?;
                }
                Ok(())
            }
        }
    }
}

#[optional_struct]
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

#[optional_struct]
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Models {
    /// Custom model configurations. Entries with the same name as a builtin will override the
    /// builtin.
    #[serde(default)]
    pub custom: Vec<ModelConfig>,

    /// Built-in model configurations.
    #[serde(default)]
    pub builtin: Vec<ModelConfig>,

    /// The default model name.
    #[serde(default)]
    pub default: String,

    /// Disable streaming for all models
    #[serde(default)]
    pub no_stream: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRoot {
    #[default]
    Discover,
    Path(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CheckMode {
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
    pub mode: CheckMode,
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
                CheckMode::Pre => crate::Mode::Pre,
                CheckMode::Post => crate::Mode::Post,
                CheckMode::Both => crate::Mode::Both,
            },
        }
    }
}

#[optional_struct(ConfigFile)]
#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct Config {
    /// Model configuration
    #[optional_rename(OptionalModels)]
    #[optional_wrap]
    pub models: Models,

    /// Which files are included by default
    pub include: Include,

    /// Glob patterns to exclude from the file list
    pub exclude: Vec<String>,

    /// The directory to store session state.
    /// The directory to store session state. Defaults to ~/.config/tenx/state
    pub session_store_dir: PathBuf,

    /// The number of times to retry a request.
    pub retry_limit: usize,

    /// The tags dialect configuration.
    #[optional_rename(OptionalTags)]
    #[optional_wrap]
    pub tags: Tags,

    /// Operations that can be executed by the model.
    #[optional_rename(OptionalOps)]
    #[optional_wrap]
    pub ops: Ops,

    /// The default context configuration.
    #[optional_rename(OptionalDefaultContext)]
    #[optional_wrap]
    pub default_context: DefaultContext,

    /// Check configuration.
    #[optional_rename(OptionalChecks)]
    #[optional_wrap]
    pub checks: Checks,

    /// Project root configuration.
    pub project_root: ProjectRoot,

    //
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
    pub(crate) test_cwd: Option<String>,
}

impl Config {
    /// Returns all model configurations, with custom models overriding built-in models with the same name.
    pub fn model_confs(&self) -> Vec<ModelConfig> {
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

        let mut model_map: HashMap<String, ModelConfig> = builtin.collect();
        model_map.extend(custom);

        model_map.into_values().collect()
    }

    pub fn cwd(&self) -> crate::Result<PathBuf> {
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
    pub fn abspath(&self, path: &Path) -> crate::Result<PathBuf> {
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
    pub fn normalize_path<P: AsRef<Path>>(&self, path: P) -> crate::Result<PathBuf> {
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
    ) -> crate::Result<PathBuf> {
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
    pub fn match_files_with_glob(&self, pattern: &str) -> crate::Result<Vec<PathBuf>> {
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

    pub fn included_files(&self) -> crate::Result<Vec<PathBuf>> {
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

    /// Serialize the Config into a RON string.
    pub fn to_ron(&self) -> crate::Result<String> {
        let pretty_config = ron::ser::PrettyConfig::default();
        ron::ser::to_string_pretty(self, pretty_config)
            .map_err(|e| TenxError::Internal(format!("Failed to serialize to RON: {}", e)))
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
    pub fn active_model(&self) -> crate::Result<crate::model::Model> {
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
            ModelConfig::Claude { api_model, key, .. } => Ok(model::Model::Claude(model::Claude {
                api_model: api_model.clone(),
                anthropic_key: key.clone(),
                streaming: !self.models.no_stream,
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
                streaming: can_stream && !self.models.no_stream,
                no_system_prompt,
            })),
        }
    }

    /// Returns the configured dialect.
    pub fn dialect(&self) -> crate::Result<crate::dialect::Dialect> {
        if let Some(dummy_dialect) = &self.dummy_dialect {
            return Ok(dialect::Dialect::Dummy(dummy_dialect.clone()));
        }
        Ok(dialect::Dialect::Tags(dialect::Tags::new(
            self.tags.smart,
            self.tags.replace,
            self.tags.udiff,
            self.ops.edit,
        )))
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

    #[test]
    fn test_config_merge() -> crate::Result<()> {
        let parsed = parse_config(
            r#"(models: (default: "foo", no_stream: true))"#,
            r#"(models: (default: "bar"), project_root: path("/foo"))"#,
        )?;
        assert_eq!(parsed.models.default, "bar");
        assert!(parsed.models.no_stream);
        assert_eq!(
            parsed.project_root,
            ProjectRoot::Path(PathBuf::from("/foo"))
        );
        Ok(())
    }

    #[test]
    fn test_config_roundtrip() -> crate::Result<()> {
        let mut config = default_config();
        config.retry_limit = 42;
        config.exclude.push("*.test".to_string());

        let ron = config.to_ron()?;
        let parsed = parse_config("", &ron)?;

        assert_eq!(parsed, config);
        Ok(())
    }

    #[test]
    fn test_parse_config_value() -> crate::Result<()> {
        // Test loading a config with a custom retry_limit
        let test_config = r#"(retry_limit: 10)"#;
        let config = parse_config("", test_config)?;
        assert_eq!(config.retry_limit, 10);

        // Test that other values remain at default
        let default_config = default_config();
        assert_eq!(config.models, default_config.models);
        assert_eq!(config.include, default_config.include);
        assert_eq!(config.exclude, default_config.exclude);

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
    fn test_included_files() -> crate::Result<()> {
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
    fn test_match_files_with_glob() -> crate::Result<()> {
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
    fn test_normalize_path_with_cwd() -> crate::Result<()> {
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
