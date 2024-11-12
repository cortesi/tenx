use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    config::Config, events::Event, model::ModelProvider, session::ModelResponse, Result, Session,
};

use std::collections::HashMap;

/// Model wrapper for OpenAI API
#[derive(Default, Debug, Clone)]
pub struct OpenAi {
    pub api_model: String,
    pub openai_key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct OpenAiUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

impl OpenAiUsage {
    pub fn values(&self) -> HashMap<String, u64> {
        let mut map = HashMap::new();
        if let Some(v) = self.prompt_tokens {
            map.insert("prompt_tokens".to_string(), v as u64);
        }
        if let Some(v) = self.completion_tokens {
            map.insert("completion_tokens".to_string(), v as u64);
        }
        if let Some(v) = self.total_tokens {
            map.insert("total_tokens".to_string(), v as u64);
        }
        map
    }
}

impl OpenAi {
    /// Creates a new OpenAi model instance
    pub fn new(api_model: String, openai_key: String) -> Result<Self> {
        unimplemented!()
    }
}

#[async_trait]
impl ModelProvider for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn send(
        &mut self,
        _config: &Config,
        _session: &Session,
        _sender: Option<mpsc::Sender<Event>>,
    ) -> Result<ModelResponse> {
        unimplemented!()
    }

    fn render(&self, _config: &Config, _session: &Session) -> Result<String> {
        unimplemented!()
    }
}
