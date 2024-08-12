use serde::{Deserialize, Serialize};

use crate::dialect::Dialects;
use crate::{Config, Operations, Prompt, Result};
use async_trait::async_trait;
use tokio::sync::mpsc;

/// A dummy model for testing purposes.
#[derive(Debug, Serialize, Deserialize)]
pub struct Dummy {
    operations: Operations,
}

impl Dummy {
    /// Creates a new Dummy model with predefined operations.
    pub fn new(operations: Operations) -> Self {
        Self { operations }
    }
}

#[async_trait]
impl super::Model for Dummy {
    async fn prompt(
        &mut self,
        _config: &Config,
        _dialect: &Dialects,
        _prompt: &Prompt,
        _sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations> {
        Ok(self.operations.clone())
    }
}

