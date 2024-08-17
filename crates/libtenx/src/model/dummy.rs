use colored::*;
use serde::{Deserialize, Serialize};

use super::ModelProvider;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{dialect::Dialect, Config, Patch, Result, Session};

/// A dummy model for testing purposes.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
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
        _dialect: &Dialect,
        _state: &Session,
        _sender: Option<mpsc::Sender<String>>,
    ) -> Result<Patch> {
        Ok(self.change_set.clone())
    }

    fn pretty_print(&self) -> String {
        format!("{}\n", "Dummy Model".bold().yellow())
    }
}
