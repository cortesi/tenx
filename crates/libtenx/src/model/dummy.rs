use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::ModelProvider;
use crate::{patch::Patch, Config, Result, Session};

/// A dummy model for testing purposes.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Dummy {
    change_set: Patch,
}

impl Dummy {
    /// Creates a new Dummy model with predefined operations.
    pub fn new(change_set: Patch) -> Self {
        Self { change_set }
    }
}

#[async_trait]
impl ModelProvider for Dummy {
    async fn prompt(
        &mut self,
        _config: &Config,
        _state: &Session,
        _sender: Option<mpsc::Sender<String>>,
    ) -> Result<Patch> {
        Ok(self.change_set.clone())
    }
}
