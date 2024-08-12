use serde::{Deserialize, Serialize};

mod claude;
mod dummy;

pub use claude::Claude;
pub use dummy::Dummy;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{dialect::Dialect, Config, Operations, PromptInput, Result};

/// Implemented by types that expose a prompt operation.
#[async_trait]
pub trait Prompt {
    async fn prompt(
        &mut self,
        config: &Config,
        dialect: &Dialect,
        prompt: &PromptInput,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations>;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Model {
    Claude(Claude),
    Dummy(Dummy),
}

#[async_trait]
impl Prompt for Model {
    async fn prompt(
        &mut self,
        config: &Config,
        dialect: &Dialect,
        prompt: &PromptInput,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations> {
        match self {
            Model::Claude(c) => c.start(config, dialect, prompt, sender).await,
            Model::Dummy(d) => d.prompt(config, dialect, prompt, sender).await,
        }
    }
}
