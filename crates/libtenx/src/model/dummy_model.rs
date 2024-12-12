use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::ModelProvider;
use crate::{
    config::Config, dialect::DialectProvider, events::Event, session::ModelResponse,
    session::Session, Result,
};

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

    fn render(&self, config: &Config, session: &Session) -> Result<String> {
        let dialect = config.dialect()?;
        let mut out = String::new();

        // Add immutable context
        out.push_str("=== Context ===\n");
        out.push_str(&dialect.render_context(config, session)?);
        out.push('\n');

        // Add request context
        for (i, step) in session.steps().iter().enumerate() {
            out.push_str(&format!("=== Step {} ===\n", i));
            out.push_str(&dialect.render_step_request(config, session, i)?);
            if let Some(_response) = &step.model_response {
                out.push_str(&dialect.render_step_response(config, session, i)?);
            }
            out.push('\n');
        }

        Ok(out)
    }
}
