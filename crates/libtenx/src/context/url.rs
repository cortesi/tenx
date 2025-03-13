use super::ContextItem;
use super::ContextProvider;
use crate::config::Config;
use crate::error::{Result, TenxError};
use crate::session::Session;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A context provider that fetches content from a remote URL.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Url {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) content: String,
}

impl Url {
    pub(crate) fn new(url: String) -> Self {
        let name = if url.len() > 40 {
            format!("{}...", &url[..37])
        } else {
            url.clone()
        };

        Self {
            name,
            url,
            content: String::new(),
        }
    }
}

#[async_trait]
impl ContextProvider for Url {
    fn context_items(&self, _config: &Config, _session: &Session) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "url".to_string(),
            source: self.url.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        format!("url: {}", self.name)
    }

    async fn refresh(&mut self, _config: &Config) -> Result<()> {
        let client = reqwest::Client::new();
        self.content = client
            .get(&self.url)
            .send()
            .await
            .map_err(|e| TenxError::Resolve(e.to_string()))?
            .text()
            .await
            .map_err(|e| TenxError::Resolve(e.to_string()))?;
        Ok(())
    }

    async fn needs_refresh(&self, _config: &Config) -> bool {
        self.content.is_empty()
    }
}
