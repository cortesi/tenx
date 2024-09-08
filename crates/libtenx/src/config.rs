use std::{
    env,
    path::{Path, PathBuf},
};

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

// Note that we can't use Optional values in the config. TOML includes no way to render
// optional values, so our strategy of rendering the full config with a default config for
// documentation falls by the wayside.

#[derive(Debug, Clone, Deserialize)]
/// Configuration for the Tenx application.
pub struct Config {
    /// Which files are included by default
    #[serde(default)]
    pub include: Include,

    /// The Anthropic API key.
    #[serde(default)]
    pub anthropic_key: String,

    /// The directory to store session state.
    #[serde(default)]
    pub session_store_dir: PathBuf,

    /// The number of times to retry a request.
    #[serde(default = "default_retry_limit")]
    pub retry_limit: usize,

    /// Skip the preflight check.
    #[serde(default)]
    pub no_preflight: bool,

    /// The default model.
    #[serde(default)]
    pub default_model: ConfigModel,

    /// The default dialect.
    #[serde(default)]
    pub default_dialect: ConfigDialect,

    /// The tags dialect configuration.
    #[serde(default)]
    pub tags: Tags,

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
        let mut state = serializer.serialize_struct("Config", 7)?;
        serialize_if_different!(state, self, default, include);
        serialize_if_different!(state, self, default, anthropic_key);
        serialize_if_different!(state, self, default, session_store_dir);
        serialize_if_different!(state, self, default, retry_limit);
        serialize_if_different!(state, self, default, no_preflight);
        serialize_if_different!(state, self, default, default_model);
        serialize_if_different!(state, self, default, default_dialect);
        serialize_if_different!(state, self, default, tags);
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
            full: false,
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
}
