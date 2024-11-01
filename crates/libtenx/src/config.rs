use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    env, fs,
    path::{absolute, Path, PathBuf},
    process::Command,
};

use globset::{Glob, GlobSetBuilder};
use pathdiff::diff_paths;
use serde::ser::SerializeStruct;

use toml;

use crate::{dialect, model, Result, TenxError};

macro_rules! serialize_if_different {
    ($state:expr, $self:expr, $default:expr, $field:ident) => {
        if $self.full || $self.$field != $default.$field {
            $state.serialize_field(stringify!($field), &$self.$field)?;
        }
    };
}

pub const HOME_CONFIG_FILE: &str = "tenx.toml";
pub const LOCAL_CONFIG_FILE: &str = ".tenx.toml";

const DEFAULT_RETRY_LIMIT: usize = 16;

fn default_retry_limit() -> usize {
    DEFAULT_RETRY_LIMIT
}

/// Returns the path to the configuration directory.
pub fn home_config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".config")
        .join("tenx")
}

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
        } else if let Ok(relative_path) = path.strip_prefix(root) {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigModel {
    #[default]
    Claude,
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
pub struct Validators {
    pub rust_cargo_check: bool,
    pub rust_cargo_test: bool,
    pub rust_cargo_clippy: bool,
    pub python_ruff_check: bool,
}

impl Default for Validators {
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
    /// The Anthropic API key.
    #[serde(default)]
    pub anthropic_key: String,

    /// The default dialect.
    #[serde(default)]
    pub default_dialect: ConfigDialect,

    /// The default model.
    #[serde(default)]
    pub default_model: ConfigModel,

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

    /// Skip the preflight check.
    #[serde(default)]
    pub no_preflight: bool,

    /// The directory to store session state.
    #[serde(default)]
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

    /// Validation configuration.
    #[serde(default)]
    pub validators: Validators,

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
        let mut state = serializer.serialize_struct("Config", 10)?;
        serialize_if_different!(state, self, default, include);
        serialize_if_different!(state, self, default, anthropic_key);
        serialize_if_different!(state, self, default, session_store_dir);
        serialize_if_different!(state, self, default, retry_limit);
        serialize_if_different!(state, self, default, no_preflight);
        serialize_if_different!(state, self, default, default_model);
        serialize_if_different!(state, self, default, default_dialect);
        serialize_if_different!(state, self, default, tags);
        serialize_if_different!(state, self, default, default_context);
        serialize_if_different!(state, self, default, validators);
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
        Self {
            include: Include::Git,
            anthropic_key: String::new(),
            session_store_dir: PathBuf::new(),
            retry_limit: DEFAULT_RETRY_LIMIT,
            no_preflight: false,
            default_model: ConfigModel::default(),
            default_dialect: ConfigDialect::default(),
            dummy_model: None,
            dummy_dialect: None,
            tags: Tags::default(),
            ops: Ops::default(),
            default_context: DefaultContext::default(),
            full: false,
            validators: Validators::default(),
            formatters: Formatters::default(),
            project_root: ProjectRoot::default(),
            test_cwd: None,
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

    pub fn session_store_dir(&self) -> PathBuf {
        if self.session_store_dir.as_os_str().is_empty() {
            home_config_dir().join("state")
        } else {
            self.session_store_dir.clone()
        }
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

    /// Normalizes a path relative to the root directory.
    /// If the path contains glob patterns ("*" or "**"), it will be returned as-is.
    pub fn normalize_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        self.normalize_path_with_cwd(path, self.cwd()?)
    }

    /// Normalizes a path relative to the root directory with a given current working directory.
    /// If the path contains glob patterns ("*" or "**"), it will be returned as-is.
    pub fn normalize_path_with_cwd<P: AsRef<Path>>(
        &self,
        path: P,
        current_dir: PathBuf,
    ) -> Result<PathBuf> {
        let path = path.as_ref();
        if path.to_str().map_or(false, |s| s.contains('*')) {
            return Ok(path.to_path_buf());
        }
        let absolute_path = if path.is_relative() {
            current_dir.join(path)
        } else {
            path.to_path_buf()
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
            .to_path_buf())
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
        match &self.include {
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

                Ok(files
                    .lines()
                    .map(|line| PathBuf::from(line.trim()))
                    .collect())
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
                Ok(included_files)
            }
        }
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
        if other.anthropic_key != dflt.anthropic_key {
            self.anthropic_key = other.anthropic_key.clone();
        }
        if other.session_store_dir != dflt.session_store_dir {
            self.session_store_dir = other.session_store_dir.clone();
        }
        if other.retry_limit != dflt.retry_limit {
            self.retry_limit = other.retry_limit;
        }
        if other.no_preflight != dflt.no_preflight {
            self.no_preflight = other.no_preflight;
        }
        if other.default_model != dflt.default_model {
            self.default_model = other.default_model.clone();
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

    /// Loads the Anthropic API key from the ANTHROPIC_API_KEY environment variable, if it exists.
    pub fn load_env(mut self) -> Self {
        if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
            self.anthropic_key = key;
        }
        self
    }

    /// Returns the configured model.
    pub fn model(&self) -> Result<crate::model::Model> {
        if let Some(dummy_model) = &self.dummy_model {
            return Ok(model::Model::Dummy(dummy_model.clone()));
        }
        match self.default_model {
            ConfigModel::Claude => Ok(model::Model::Claude(model::Claude {})),
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
        set_config!(config, anthropic_key, "test_key".to_string());
        set_config!(config, session_store_dir, PathBuf::from("/tmp/test"));
        set_config!(config, retry_limit, 5);
        set_config!(config, no_preflight, true);
        set_config!(config, tags.smart, false);
        set_config!(config, default_model, ConfigModel::Claude);
        set_config!(config, default_dialect, ConfigDialect::Tags);

        let toml_str = config.to_toml().unwrap();

        let deserialized_config = Config::from_toml(&toml_str).unwrap();

        assert_eq!(config.anthropic_key, deserialized_config.anthropic_key);
        assert_eq!(
            config.session_store_dir,
            deserialized_config.session_store_dir
        );
        assert_eq!(config.retry_limit, deserialized_config.retry_limit);
        assert_eq!(config.no_preflight, deserialized_config.no_preflight);
        assert_eq!(config.default_model, deserialized_config.default_model);
        assert_eq!(config.default_dialect, deserialized_config.default_dialect);
        assert_eq!(config.tags.smart, deserialized_config.tags.smart);

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
    }

    #[test]
    fn test_config_merge() {
        let mut base_config = Config::default();
        set_config!(base_config, anthropic_key, "base_key".to_string());
        set_config!(base_config, retry_limit, 5);

        let mut other_config = Config::default();
        set_config!(other_config, anthropic_key, "other_key".to_string());
        set_config!(other_config, session_store_dir, PathBuf::from("/tmp/other"));
        set_config!(other_config, no_preflight, true);
        set_config!(
            other_config,
            include,
            Include::Glob(vec!["*.rs".to_string()])
        );

        base_config.merge(&other_config);

        assert_eq!(base_config.anthropic_key, "other_key".to_string());
        assert_eq!(base_config.session_store_dir, PathBuf::from("/tmp/other"));
        assert_eq!(base_config.retry_limit, 5);
        assert!(base_config.no_preflight);
        assert_eq!(base_config.default_model, ConfigModel::Claude);
        assert_eq!(base_config.default_dialect, ConfigDialect::Tags);
        assert!(!base_config.tags.smart);
        assert!(matches!(base_config.include, Include::Glob(_)));
        if let Include::Glob(patterns) = &base_config.include {
            assert_eq!(patterns, &vec!["*.rs".to_string()]);
        }
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
            config_without_change.session_store_dir(),
            home_config_dir().join("state")
        );
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
        let toml_str = "retry_limit = 42";
        let config = Config::from_toml(toml_str).unwrap();

        assert_eq!(config.retry_limit, 42);
        assert_eq!(config.anthropic_key, "");
        assert_eq!(config.session_store_dir, PathBuf::new());
        assert!(!config.no_preflight);
        assert_eq!(config.default_model, ConfigModel::Claude);
        assert_eq!(config.default_dialect, ConfigDialect::Tags);
        assert!(!config.tags.smart);
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
            ],
        )?;

        let config = Config {
            include: Include::Glob(vec!["*.rs".to_string(), "subdir/*.txt".to_string()]),
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

        let root = project.tempdir.path().to_path_buf();
        let sub_dir = root.join("subdir");
        let outside_dir = root.parent().unwrap().join("outside");

        // Test 1: Current dir is the root directory
        let result = project
            .config
            .normalize_path_with_cwd("file.txt", root.clone())?;
        assert_eq!(result, PathBuf::from("file.txt"));

        // Test 2: Current dir is under the root directory
        let result = project
            .config
            .normalize_path_with_cwd("subfile.txt", sub_dir.clone())?;
        assert_eq!(result, PathBuf::from("subdir/subfile.txt"));

        // Test 3: Current dir is outside the root directory
        let result = project
            .config
            .normalize_path_with_cwd("outsidefile.txt", outside_dir.clone())?;
        let expected = outside_dir
            .join("outsidefile.txt")
            .strip_prefix(&root)
            .unwrap_or(&outside_dir.join("outsidefile.txt"))
            .to_path_buf();
        assert_eq!(
            result.canonicalize().unwrap(),
            expected.canonicalize().unwrap()
        );

        // Test 4: Absolute path
        let abs_path = root.join("abs_file.txt");
        let result = project
            .config
            .normalize_path_with_cwd(&abs_path, root.clone())?;
        assert_eq!(result, PathBuf::from("abs_file.txt"));

        Ok(())
    }
}
