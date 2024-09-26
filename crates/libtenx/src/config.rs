use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use globset::{Glob, GlobSetBuilder};
use serde::{
    ser::{SerializeStruct, Serializer},
    Deserialize, Serialize,
};

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

/// Finds the root directory based on a specified working directory or git repo root.
pub fn find_project_root(current_dir: &Path) -> PathBuf {
    let mut dir = current_dir.to_path_buf();
    loop {
        if dir.join(".git").is_dir() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    current_dir.to_path_buf()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub struct DefaultContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ruskel: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
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

    /// The default context configuration.
    #[serde(default)]
    pub default_context: DefaultContext,

    /// Validation configuration.
    #[serde(default)]
    pub validators: Validators,

    /// Formatting configuration.
    #[serde(default)]
    pub formatters: Formatters,

    /// Set a dummy model for end-to-end testing. Over-rides the configured model.
    #[serde(skip)]
    dummy_model: Option<model::DummyModel>,

    /// Set a dummy dialect for end-to-end testing. Over-rides the configured dialect.
    #[serde(skip)]
    dummy_dialect: Option<dialect::DummyDialect>,

    /// When true, serializes all fields regardless of default values.
    #[serde(skip)]
    full: bool,
}

impl Serialize for Config {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let default = Config::default();
        let mut state = serializer.serialize_struct("Config", 9)?;
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
        state.end()
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
            default_context: DefaultContext::default(),
            full: false,
            validators: Validators::default(),
            formatters: Formatters::default(),
        }
    }
}

impl Config {
    pub fn session_store_dir(&self) -> PathBuf {
        if self.session_store_dir.as_os_str().is_empty() {
            home_config_dir().join("state")
        } else {
            self.session_store_dir.clone()
        }
    }

    pub fn included_files(&self, project_root: &Path) -> Result<Vec<PathBuf>> {
        match &self.include {
            Include::Git => {
                let output = Command::new("git")
                    .arg("ls-files")
                    .current_dir(project_root)
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
                walk_directory(project_root, project_root, &globset, &mut included_files)?;
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
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
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
        println!("Serialized TOML:\n{}", toml_str);

        let deserialized_config = Config::from_toml(&toml_str).unwrap();
        println!("Deserialized Config:\n{:#?}", deserialized_config);

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
        println!("Default Config TOML:\n{}", default_toml_str);

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
        println!("Serialized TOML:\n{}", toml_str);

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
    fn test_included_files() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Create dummy files
        File::create(root_path.join("file1.rs")).unwrap();
        File::create(root_path.join("file2.txt")).unwrap();
        std::fs::create_dir(root_path.join("subdir")).unwrap();
        File::create(root_path.join("subdir").join("file3.rs")).unwrap();
        File::create(root_path.join("subdir").join("file4.txt")).unwrap();

        let config = Config {
            include: Include::Glob(vec!["*.rs".to_string(), "subdir/*.txt".to_string()]),
            ..Default::default()
        };

        let included_files = config.included_files(root_path).unwrap();

        let mut expected_files: Vec<PathBuf> = vec![
            PathBuf::from("file1.rs"),
            PathBuf::from("subdir/file3.rs"),
            PathBuf::from("subdir/file4.txt"),
        ];
        expected_files.sort();

        let mut included_files: Vec<PathBuf> = included_files.into_iter().collect();
        included_files.sort();

        assert_eq!(included_files, expected_files);
    }
}
