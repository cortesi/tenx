use super::ContextItem;
use super::ContextProvider;
use crate::config::Config;
use crate::error::{Result, TenxError};
use crate::session::Session;
use async_trait::async_trait;
use libruskel::Ruskel as LibRuskel;
use serde::{Deserialize, Serialize};

/// A context provider that generates Rust API documentation using Ruskel.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Ruskel {
    pub(crate) name: String,
    pub(crate) content: String,
}

impl Ruskel {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            content: String::new(),
        }
    }
}

#[async_trait]
impl ContextProvider for Ruskel {
    fn context_items(&self, _config: &Config, _session: &Session) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "ruskel".to_string(),
            source: self.name.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        format!("ruskel: {}", self.name)
    }

    fn id(&self) -> String {
        self.name.clone()
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        let ruskel = LibRuskel::new(&self.name);
        self.content = ruskel
            .render(false, false, true)
            .map_err(|e| TenxError::Resolve(e.to_string()))?;
        Ok(())
    }

    async fn needs_refresh(&self, _config: &Config) -> bool {
        self.content.is_empty()
    }
}
