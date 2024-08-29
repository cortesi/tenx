use std::path::{Path, PathBuf};

use crate::{model, Result, TenxError};

#[derive(Debug, Clone)]
pub struct Config {
    pub anthropic_key: String,
    pub session_store_dir: Option<PathBuf>,
    pub retry_limit: usize,
    pub no_preflight: bool,
    pub model: Option<model::Model>,
    pub dialect: Option<crate::dialect::Dialect>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_key: String::new(),
            session_store_dir: None,
            retry_limit: 10,
            no_preflight: false,
            model: None,
            dialect: None,
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
    pub fn with_model(mut self, model: model::Model) -> Self {
        self.model = Some(model);
        self
    }

    /// Sets the configured dialect
    pub fn with_dialect(mut self, dialect: crate::dialect::Dialect) -> Self {
        self.dialect = Some(dialect);
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
        self.model
            .clone()
            .ok_or_else(|| TenxError::Internal("Model not configured".to_string()))
    }

    /// Returns the configured dialect.
    pub fn dialect(&self) -> Result<crate::dialect::Dialect> {
        self.dialect
            .clone()
            .ok_or_else(|| TenxError::Internal("Dialect not configured".to_string()))
    }
}
