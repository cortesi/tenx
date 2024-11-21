mod claude;
mod conversation;
mod dummy_model;
pub mod openai;

pub use openai::OPENAI_API_BASE;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub use claude::{Claude, ClaudeUsage};
pub use dummy_model::{DummyModel, DummyUsage};
pub use openai::{OpenAi, OpenAiUsage};

use crate::{config::Config, events::Event, session::ModelResponse, Result, Session};

use std::collections::HashMap;

/// Represents usage statistics for different model types.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Usage {
    Claude(ClaudeUsage),
    OpenAi(OpenAiUsage),
    Dummy(DummyUsage),
}

impl Usage {
    /// Returns a map of usage statistics.
    pub fn values(&self) -> HashMap<String, u64> {
        match self {
            Usage::Claude(usage) => usage.values(),
            Usage::OpenAi(usage) => usage.values(),
            Usage::Dummy(usage) => usage.values(),
        }
    }

    /// Returns a tuple of (tokens in, tokens out).
    pub fn totals(&self) -> (u64, u64) {
        match self {
            Usage::Claude(usage) => usage.totals(),
            Usage::OpenAi(usage) => usage.totals(),
            Usage::Dummy(usage) => usage.totals(),
        }
    }
}

/// Implemented by types that expose a prompt operation.
#[async_trait]
pub trait ModelProvider {
    /// Returns the name of the model provider.
    fn name(&self) -> String;

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
    OpenAi(OpenAi),
    Dummy(DummyModel),
}

#[async_trait]
impl ModelProvider for Model {
    fn name(&self) -> String {
        match self {
            Model::Claude(c) => c.name(),
            Model::OpenAi(o) => o.name(),
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
            Model::OpenAi(o) => o.send(config, session, sender).await,
            Model::Dummy(d) => d.send(config, session, sender).await,
        }
    }

    fn render(&self, config: &Config, session: &Session) -> Result<String> {
        match self {
            Model::Claude(c) => c.render(config, session),
            Model::OpenAi(o) => o.render(config, session),
            Model::Dummy(d) => d.render(config, session),
        }
    }
}
