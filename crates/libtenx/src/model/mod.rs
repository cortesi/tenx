//! Model abstractions and implementations for interacting with AI language models.
//!
//! This module provides traits and implementations for different AI model providers,
//! along with usage tracking and response handling.

mod claude;
mod claude_editor;
mod dummy_model;
mod google;
mod openai;
mod tags;

use std::collections::HashMap;

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

use state::{Patch, PatchFailure};

use crate::{
    checks::CheckResult, context::ContextItem, error::Result, events::EventSender,
    session::ModelResponse,
};

pub use claude::{Claude, ClaudeChat, ClaudeUsage};
pub use claude_editor::ClaudeEditor;
pub use dummy_model::{DummyModel, DummyUsage};
pub use google::{Google, GoogleChat, GoogleUsage};
pub use openai::{OpenAi, OpenAiChat, OpenAiUsage, ReasoningEffort};

/// A trait used to prepare a chat interaction to be sent to the model for
/// completion.
///
/// Calls to `add_user_message` and `add_agent_message` must be interleaved, with user messages
/// first.
#[async_trait]
pub trait Chat: Send {
    /// Adds to the system prompt for the chat. May be called multiple times, but all calls must be
    /// at the start of the chat. Typically, this appends to the model system prompt that defines
    /// basic interactions.
    fn add_system_prompt(&mut self, prompt: &str) -> Result<()>;

    /// Adds a raw  user message to the chat. If the last message is a user message, the text is
    /// appended. If the last message is an agent message, a new user message is started.
    fn add_user_message(&mut self, text: &str) -> Result<()>;

    /// Adds a raw agent message to the chat. If the last message is an agent message, the text is
    /// appended. If the last message is an user message, a new agent message is started.
    fn add_agent_message(&mut self, text: &str) -> Result<()>;

    /// Adds immutable context data to the chat, can be called multiple times, at any time.
    /// May start a new user message and synthesize an agent response.
    fn add_context(&mut self, ctx: &ContextItem) -> Result<()>;

    /// Adds editable data to the chat. Can be called multiple times, at any time.
    /// May start a new user message and synthesize an agent response.
    fn add_editable(&mut self, path: &str, data: &str) -> Result<()>;

    /// Adds a representation of a patch to the chat.
    fn add_agent_patch(&mut self, _patch: &Patch) -> Result<()> {
        Ok(())
    }

    /// Adds a representation of a patch to the chat.
    fn add_agent_comment(&mut self, _patch: &str) -> Result<()> {
        Ok(())
    }

    /// Adds a representation of a patch to the chat.
    fn add_user_prompt(&mut self, _patch: &str) -> Result<()> {
        Ok(())
    }

    fn add_user_patch_failure(&mut self, _patch: &[PatchFailure]) -> Result<()> {
        Ok(())
    }

    /// Adds check results to the current user message, or creates a new user message.
    fn add_user_check_results(&mut self, _results: &[CheckResult]) -> Result<()> {
        Ok(())
    }

    /// Render and send a session to the model.
    async fn send(&mut self, sender: Option<EventSender>) -> Result<ModelResponse>;

    /// Render the chat for debugging. Often this is the JSON serialization of the message
    /// as it would be sent to the model.
    fn render(&self) -> Result<String>;
}

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
#[enum_dispatch(Model)]
pub trait ModelProvider {
    /// Returns user-facing name of the model.
    fn name(&self) -> String;

    /// Returns underlying name of the model.
    fn api_model(&self) -> String;

    /// Return a conversation object for the model. If the model does not support
    /// chat interactions, this should return `None`.
    fn chat(&self) -> Option<Box<dyn Chat>> {
        None
    }
}

/// Available model implementations that can be used for AI interactions.
#[enum_dispatch]
#[derive(Debug, Clone)]
pub enum Model {
    Claude(Claude),
    ClaudeEditor(ClaudeEditor),
    OpenAi(OpenAi),
    Google(google::Google),
    Dummy(DummyModel),
}
