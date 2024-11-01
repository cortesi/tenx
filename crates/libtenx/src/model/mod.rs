mod claude;
mod dummy_model;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub use claude::{Claude, ClaudeUsage};
pub use dummy_model::{DummyModel, DummyUsage};

use crate::{config::Config, events::Event, session::ModelResponse, Result, Session};

use std::collections::HashMap;

/// Represents usage statistics for different model types.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Usage {
    Claude(ClaudeUsage),
    Dummy(DummyUsage),
}

impl Usage {
    /// Returns a map of usage statistics.
    pub fn values(&self) -> HashMap<String, u64> {
        match self {
            Usage::Claude(usage) => usage.values(),
            Usage::Dummy(usage) => usage.values(),
        }
    }
}

/// Implemented by types that expose a prompt operation.
#[async_trait]
pub trait ModelProvider {
    /// Returns the name of the model provider.
    fn name(&self) -> &'static str;

    /// Render and send a session to the model.
    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<ModelResponse>;

    /// Render a session for display to the user.
    fn render(&self, config: &Config, session: &Session) -> Result<String>;
}

#[derive(Debug, Clone)]
pub enum Model {
    Claude(Claude),
    Dummy(DummyModel),
}

#[async_trait]
impl ModelProvider for Model {
    fn name(&self) -> &'static str {
        match self {
            Model::Claude(c) => c.name(),
            Model::Dummy(d) => d.name(),
        }
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<ModelResponse> {
        match self {
            Model::Claude(c) => c.send(config, session, sender).await,
            Model::Dummy(d) => d.send(config, session, sender).await,
        }
    }

    fn render(&self, config: &Config, session: &Session) -> Result<String> {
        match self {
            Model::Claude(c) => c.render(config, session),
            Model::Dummy(d) => d.render(config, session),
        }
    }
}
