use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Chat, ModelProvider};
use crate::{context::ContextItem, error::Result, events::EventSender, session::ModelResponse};

use std::collections::HashMap;

/// A dummy chat implementation for testing purposes.
pub struct DummyChat {
    model_response: Result<ModelResponse>,
}

#[async_trait]
impl Chat for DummyChat {
    fn add_system_prompt(&mut self, _prompt: &str) -> Result<()> {
        Ok(())
    }

    fn add_user_message(&mut self, _text: &str) -> Result<()> {
        Ok(())
    }

    fn add_agent_message(&mut self, _text: &str) -> Result<()> {
        Ok(())
    }

    fn add_context(&mut self, _ctx: &ContextItem) -> Result<()> {
        Ok(())
    }

    fn add_editable(&mut self, _path: &str, _data: &str) -> Result<()> {
        Ok(())
    }

    async fn send(&mut self, _sender: Option<EventSender>) -> Result<ModelResponse> {
        let mut resp = self.model_response.clone()?;
        resp.usage = Some(super::Usage::Dummy(DummyUsage { dummy_counter: 1 }));
        Ok(resp)
    }

    fn render(&self) -> Result<String> {
        Ok("DummyChat render".to_string())
    }
}

/// A dummy usage struct for testing purposes.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct DummyUsage {
    pub dummy_counter: u32,
}

impl DummyUsage {
    pub fn values(&self) -> HashMap<String, u64> {
        let mut map = HashMap::new();
        map.insert("dummy_counter".to_string(), self.dummy_counter as u64);
        map
    }

    pub fn totals(&self) -> (u64, u64) {
        (self.dummy_counter as u64, self.dummy_counter as u64)
    }
}

/// A dummy model for testing purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DummyModel {
    model_response: Result<ModelResponse>,
}

impl DummyModel {
    /// Creates a new Dummy model with predefined operations.
    pub fn from_model_response(mr: ModelResponse) -> Self {
        Self {
            model_response: Ok(mr),
        }
    }
}

impl Default for DummyModel {
    fn default() -> Self {
        Self {
            model_response: Ok(ModelResponse::default()),
        }
    }
}

#[async_trait]
impl ModelProvider for DummyModel {
    fn name(&self) -> String {
        "dummy".to_string()
    }

    fn api_model(&self) -> String {
        "dummy".to_string()
    }

    fn chat(&self) -> Option<Box<dyn Chat>> {
        Some(Box::new(DummyChat {
            model_response: self.model_response.clone(),
        }))
    }
}
