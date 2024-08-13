use serde::{Deserialize, Serialize};

mod claude;
mod dummy;

pub use claude::Claude;
pub use dummy::Dummy;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{dialect::Dialect, Config, Operations, Result, State};

/// Implemented by types that expose a prompt operation.
#[async_trait]
pub trait ModelProvider {
    async fn prompt(
        &mut self,
        config: &Config,
        dialect: &Dialect,
        state: &State,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations>;

    fn pretty_print(&self) -> String;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Model {
    Claude(Claude),
    Dummy(Dummy),
}

#[async_trait]
impl ModelProvider for Model {
    async fn prompt(
        &mut self,
        config: &Config,
        dialect: &Dialect,
        state: &State,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations> {
        match self {
            Model::Claude(c) => c.prompt(config, dialect, state, sender).await,
            Model::Dummy(d) => d.prompt(config, dialect, state, sender).await,
        }
    }

    fn pretty_print(&self) -> String {
        match self {
            Model::Claude(c) => c.pretty_print(),
            Model::Dummy(d) => d.pretty_print(),
        }
    }
}
