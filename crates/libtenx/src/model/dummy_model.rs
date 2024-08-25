use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::ModelProvider;
use crate::{events::Event, patch::Patch, Config, Result, Session};

/// A dummy model for testing purposes.
#[derive(Debug, Serialize, Deserialize)]
pub struct DummyModel {
    change_set: Result<Patch>,
}

impl DummyModel {
    /// Creates a new Dummy model with predefined operations.
    pub fn from_patch(change_set: Patch) -> Self {
        Self {
            change_set: Ok(change_set),
        }
    }
}

impl Default for DummyModel {
    fn default() -> Self {
        Self {
            change_set: Ok(Patch::default()),
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
    ) -> Result<Patch> {
        self.change_set.clone()
    }

    fn render(&self, _session: &Session) -> Result<String> {
        Ok("Dummy model render".to_string())
    }
}