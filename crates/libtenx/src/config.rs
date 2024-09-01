use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use toml;

use crate::{dialect, model, Result, TenxError};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum ConfigModel {
    #[default]
    Claude,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum ConfigDialect {
    #[default]
    Tags,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tags {
    pub smart: bool,
}

impl Default for Tags {
    fn default() -> Self {
        Self { smart: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// The Anthropic API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_key: Option<String>,

    /// The directory to store session state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_store_dir: Option<PathBuf>,

    /// The number of times to retry a request.
    #[serde(skip_serializing_if = "is_default_retry_limit")]
    pub retry_limit: usize,

    /// Skip the preflight check.
    #[serde(skip_serializing_if = "default")]
    pub no_preflight: bool,

    /// The default model.
    #[serde(default, skip_serializing_if = "default")]
    pub default_model: ConfigModel,

    /// The default dialect.
    #[serde(default, skip_serializing_if = "default")]
    pub default_dialect: ConfigDialect,

    /// The tags dialect configuration.
    #[serde(skip_serializing_if = "default")]
    pub tags: Tags,

    /// Set a dummy model for end-to-end testing. Over-rides the configured model.
    #[serde(skip_serializing, skip_deserializing)]
    dummy_model: Option<model::DummyModel>,

    /// Set a dummy dialect for end-to-end testing. Over-rides the configured dialect.
    #[serde(skip_serializing, skip_deserializing)]
    dummy_dialect: Option<dialect::DummyDialect>,
}

fn default<T: Default + PartialEq>(t: &T) -> bool {
    *t == Default::default()
}

fn is_default_retry_limit(value: &usize) -> bool {
    *value == Config::default().retry_limit
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_key: None,
            session_store_dir: None,
            retry_limit: 10,
            no_preflight: false,
            default_model: ConfigModel::default(),
            default_dialect: ConfigDialect::default(),
            dummy_model: None,
            dummy_dialect: None,
            tags: Tags::default(),
        }
    }
}

impl Config {
    /// Deserialize a TOML string into a Config.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str)
            .map_err(|e| TenxError::Internal(format!("Failed to parse TOML: {}", e)))
    }

    /// Serialize the Config into a TOML string.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self)
            .map_err(|e| TenxError::Internal(format!("Failed to serialize to TOML: {}", e)))
    }

    /// Sets the Anthropic API key. If None, the existing value is unchanged.
    pub fn with_anthropic_key(mut self, key: Option<String>) -> Self {
        if let Some(key) = key {
            self.anthropic_key = Some(key);
        }
        self
    }

    /// Sets the configured model
    pub fn with_default_model(mut self, model: ConfigModel) -> Self {
        self.default_model = model;
        self
    }

    pub fn with_dummy_model(mut self, model: model::DummyModel) -> Self {
        self.dummy_model = Some(model);
        self
    }

    pub fn with_dummy_dialect(mut self, dialect: dialect::DummyDialect) -> Self {
        self.dummy_dialect = Some(dialect);
        self
    }

    /// Sets the configured dialect
    pub fn with_default_dialect(mut self, dialect: ConfigDialect) -> Self {
        self.default_dialect = dialect;
        self
    }

    /// Sets the state directory.
    pub fn with_session_store_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.session_store_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sets the retry limit.
    pub fn with_retry_limit(mut self, limit: usize) -> Self {
        self.retry_limit = limit;
        self
    }

    /// Sets the no_preflight flag.
    pub fn with_no_preflight(mut self, no_preflight: bool) -> Self {
        self.no_preflight = no_preflight;
        self
    }

    /// Sets the smart flag for the Tags dialect.
    pub fn with_tags_smart(mut self, smart: bool) -> Self {
        self.tags.smart = smart;
        self
    }

    /// Loads the Anthropic API key from the ANTHROPIC_API_KEY environment variable, if it exists.
    pub fn load_env(mut self) -> Self {
        if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
            self.anthropic_key = Some(key);
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
            ConfigDialect::Tags => Ok(dialect::Dialect::Tags(dialect::Tags {
                smart: self.tags.smart,
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_serialization() {
        let config = Config::default()
            .with_anthropic_key(Some("test_key".to_string()))
            .with_session_store_dir("/tmp/test")
            .with_retry_limit(5)
            .with_no_preflight(true)
            .with_tags_smart(false)
            .with_default_model(ConfigModel::Claude)
            .with_default_dialect(ConfigDialect::Tags);

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
}
