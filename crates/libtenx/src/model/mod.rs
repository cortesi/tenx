use serde::{Deserialize, Serialize};

mod claude;
mod dummy;

pub use claude::Claude;
pub use dummy::Dummy;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{dialect::Dialects, Config, Operations, Prompt, Result};

#[async_trait]
pub trait Model {
    async fn prompt(
        &mut self,
        config: &Config,
        dialect: &Dialects,
        prompt: &Prompt,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations>;
}

#[async_trait]
impl Model for Models {
    async fn prompt(
        &mut self,
        config: &Config,
        dialect: &Dialects,
        prompt: &Prompt,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations> {
        match self {
            Models::Claude(c) => c.start(config, dialect, prompt, sender).await,
            Models::Dummy(d) => d.prompt(config, dialect, prompt, sender).await,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Models {
    Claude(Claude),
    Dummy(Dummy),
}

