use colored::*;
use serde::{Deserialize, Serialize};

use super::ModelProvider;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{dialect::Dialect, Config, Operations, Result, State};

/// A dummy model for testing purposes.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
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
impl ModelProvider for Dummy {
    async fn prompt(
        &mut self,
        _config: &Config,
        _dialect: &Dialect,
        _state: &State,
        _sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations> {
        Ok(self.operations.clone())
    }

    fn pretty_print(&self) -> String {
        format!("{}\n", "Dummy Model".bold().yellow())
    }
}

