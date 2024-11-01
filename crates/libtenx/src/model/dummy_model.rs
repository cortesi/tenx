use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::ModelProvider;
use crate::{config::Config, events::Event, ModelResponse, Result, Session};

use std::collections::HashMap;

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
}

/// A dummy model for testing purposes.
#[derive(Debug, Clone)]
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
    fn name(&self) -> &'static str {
        "dummy"
    }

    async fn send(
        &mut self,
        _config: &Config,
        _state: &Session,
        _sender: Option<mpsc::Sender<Event>>,
    ) -> Result<ModelResponse> {
        let mut resp = self.model_response.clone()?;
        resp.usage = Some(super::Usage::Dummy(DummyUsage { dummy_counter: 1 }));
        Ok(resp)
    }

    fn render(&self, _conf: &Config, _session: &Session) -> Result<String> {
        Ok("Dummy model render".to_string())
    }
}
