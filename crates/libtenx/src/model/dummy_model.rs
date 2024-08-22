use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::ModelProvider;
use crate::{patch::Patch, Config, Result, Session};

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
    async fn prompt(
        &mut self,
        _config: &Config,
        _state: &Session,
        _sender: Option<mpsc::Sender<String>>,
    ) -> Result<Patch> {
        self.change_set.clone()
    }
}
