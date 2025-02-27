//! Model abstractions and implementations for interacting with AI language models.
//!
//! This module provides traits and implementations for different AI model providers,
//! along with usage tracking and response handling.

mod claude;
mod conversation;
mod dummy_model;
mod google;
mod openai;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use claude::{Claude, ClaudeUsage};
pub use dummy_model::{DummyModel, DummyUsage};
pub use google::{Google, GoogleUsage};
pub use openai::{OpenAi, OpenAiUsage, ReasoningEffort};

use crate::{
    config::Config,
    events::EventSender,
    session::{ModelResponse, Session},
    Result,
};

use std::collections::HashMap;

/// Represents usage statistics for different model types.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Usage {
    Claude(ClaudeUsage),
    OpenAi(OpenAiUsage),
    Dummy(DummyUsage),
    Google(google::GoogleUsage),
}

impl Usage {
    /// Returns a map of usage statistics.
    pub fn values(&self) -> HashMap<String, u64> {
        match self {
            Usage::Claude(usage) => usage.values(),
            Usage::OpenAi(usage) => usage.values(),
            Usage::Dummy(usage) => usage.values(),
            Usage::Google(usage) => usage.values(),
        }
    }

    /// Returns a tuple of (tokens in, tokens out).
    pub fn totals(&self) -> (u64, u64) {
        match self {
            Usage::Claude(usage) => usage.totals(),
            Usage::OpenAi(usage) => usage.totals(),
            Usage::Dummy(usage) => usage.totals(),
            Usage::Google(usage) => usage.totals(),
        }
    }
}

/// Implemented by types that expose a prompt operation.
#[async_trait]
pub trait ModelProvider {
    /// Returns user-facing name of the model.
    fn name(&self) -> String;

    /// Returns underlying name of the model.
    fn api_model(&self) -> String;

    /// Render and send a session to the model.
    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<EventSender>,
    ) -> Result<ModelResponse>;

    /// Render a session as it would be sent to the model. It's a requirement that this step be
    /// able to render a sessio with no steps, that is, with the system prompt only.
    fn render(&self, config: &Config, session: &Session) -> Result<String>;
}

/// Available model implementations that can be used for AI interactions.
#[derive(Debug, Clone)]
pub enum Model {
    Claude(Claude),
    OpenAi(OpenAi),
    Dummy(DummyModel),
    Google(google::Google),
}

#[async_trait]
impl ModelProvider for Model {
    fn name(&self) -> String {
        match self {
            Model::Claude(c) => c.name(),
            Model::OpenAi(o) => o.name(),
            Model::Dummy(d) => d.name(),
            Model::Google(g) => g.name(),
        }
    }

    fn api_model(&self) -> String {
        match self {
            Model::Claude(c) => c.api_model(),
            Model::OpenAi(o) => o.api_model(),
            Model::Dummy(d) => d.api_model(),
            Model::Google(g) => g.api_model(),
        }
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<EventSender>,
    ) -> Result<ModelResponse> {
        match self {
            Model::Claude(c) => c.send(config, session, sender).await,
            Model::OpenAi(o) => o.send(config, session, sender).await,
            Model::Dummy(d) => d.send(config, session, sender).await,
            Model::Google(g) => g.send(config, session, sender).await,
        }
    }

    fn render(&self, config: &Config, session: &Session) -> Result<String> {
        match self {
            Model::Claude(c) => c.render(config, session),
            Model::OpenAi(o) => o.render(config, session),
            Model::Dummy(d) => d.render(config, session),
            Model::Google(g) => g.render(config, session),
        }
    }
}
