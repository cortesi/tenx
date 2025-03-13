use super::ContextItem;
use super::ContextProvider;
use crate::config::Config;
use crate::error::Result;
use crate::session::Session;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A context provider for raw text content.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Text {
    pub(crate) name: String,
    pub(crate) content: String,
}

impl Text {
    pub(crate) fn new(name: String, content: String) -> Self {
        Self { name, content }
    }
}

#[async_trait]
impl ContextProvider for Text {
    fn context_items(&self, _config: &Config, _session: &Session) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "text".to_string(),
            source: self.name.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        let lines = self.content.lines().count();
        let chars = self.content.chars().count();
        format!("text: {} ({} lines, {} chars)", self.name, lines, chars)
    }

    fn id(&self) -> String {
        self.name.clone()
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        Ok(())
    }
}
