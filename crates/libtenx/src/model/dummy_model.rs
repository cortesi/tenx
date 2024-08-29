use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::ModelProvider;
use crate::{events::Event, patch::Patch, Config, Result, Session};

use std::collections::HashMap;

/// A dummy usage struct for testing purposes.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
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
    ) -> Result<(Patch, super::Usage)> {
        let patch = self.change_set.clone()?;
        let usage = super::Usage::Dummy(DummyUsage { dummy_counter: 1 });
        Ok((patch, usage))
    }

    fn render(&self, _session: &Session) -> Result<String> {
        Ok("Dummy model render".to_string())
    }
}
