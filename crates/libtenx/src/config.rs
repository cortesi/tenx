use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{dialect, model, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ConfigModel {
    #[default]
    Claude,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ConfigDialect {
    #[default]
    Tags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub anthropic_key: String,
    pub session_store_dir: Option<PathBuf>,
    pub retry_limit: usize,
    pub no_preflight: bool,
    pub default_model: ConfigModel,
    pub default_dialect: ConfigDialect,
    pub tags: Tags,

    /// Set a dummy model for end-to-end testing. Over-rides the configured model.
    #[serde(skip_serializing, skip_deserializing)]
    dummy_model: Option<model::DummyModel>,

    /// Set a dummy dialect for end-to-end testing. Over-rides the configured dialect.
    #[serde(skip_serializing, skip_deserializing)]
    dummy_dialect: Option<dialect::DummyDialect>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_key: String::new(),
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
    /// Sets the Anthropic API key.
    pub fn with_anthropic_key(mut self, key: String) -> Self {
        self.anthropic_key = key;
        self
    }

    /// Sets the configured model
    pub fn with_model(mut self, model: ConfigModel) -> Self {
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
    pub fn with_dialect(mut self, dialect: ConfigDialect) -> Self {
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
