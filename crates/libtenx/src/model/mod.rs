use serde::{Deserialize, Serialize};

mod claude;
mod dummy;

pub use claude::Claude;
pub use dummy::Dummy;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{patch::Patch, Config, Result, Session};

/// Implemented by types that expose a prompt operation.
#[async_trait]
pub trait ModelProvider {
    async fn prompt(
        &mut self,
        config: &Config,
        state: &Session,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Patch>;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Model {
    Claude(Claude),
    Dummy(Dummy),
}

#[async_trait]
impl ModelProvider for Model {
    async fn prompt(
        &mut self,
        config: &Config,
        state: &Session,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Patch> {
        match self {
            Model::Claude(c) => c.prompt(config, state, sender).await,
            Model::Dummy(d) => d.prompt(config, state, sender).await,
        }
    }
}
