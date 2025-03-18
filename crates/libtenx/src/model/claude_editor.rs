//! This module implements the Claude model provider for the tenx system.
use misanthropy::{tools, Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde_json;
use tracing::{trace, warn};

use super::claude::ClaudeUsage;
use crate::{
    config::Config,
    dialect::Dialect,
    error::{Result, TenxError},
    events::*,
    model::conversation::{build_conversation, Conversation},
    model::ModelProvider,
    session::ModelResponse,
    session::Session,
    throttle::Throttle,
};

const MAX_TOKENS: u32 = 8192;

/// A model that interacts with the Anthropic API. The general design of the model is to:
///
/// - Have a large, cached system prompt with many examples.
/// - Emit both the non-editable context and the editable context as pre-primed messages in the
///   prompt.
/// - Edit the conversation to keep the most up-to-date editable files frontmost.
#[derive(Default, Debug, Clone)]
pub struct ClaudeEditor {
    /// The user facing name of the model
    pub name: String,
    /// Upstream model name to use
    pub api_model: String,
    /// The Anthropic API key
    pub anthropic_key: String,
    /// Whether to stream responses
    pub streaming: bool,
}

impl ClaudeEditor {
    async fn stream_response(
        &mut self,
        api_key: String,
        req: &misanthropy::MessagesRequest,
        sender: Option<EventSender>,
    ) -> Result<misanthropy::MessagesResponse> {
        let anthropic = Anthropic::new(&api_key);
        let mut streamed_response = anthropic.messages_stream(req)?;
        while let Some(event) = streamed_response.next().await {
            let event = event?;
            match event {
                StreamEvent::ContentBlockDelta {
                    delta: ContentBlockDelta::TextDelta { text },
                    ..
                } => {
                    send_event(&sender, Event::Snippet(text))?;
                }
                StreamEvent::Error { error } => {
                    warn!("Error in stream: {:?}", error);
                }
                StreamEvent::MessageStop => {
                    // The message has ended, but we don't need to do anything special here
                }
                _ => {} // Ignore other event types
            }
        }
        Ok(streamed_response.response)
    }

    fn extract_changes(&self, req: &misanthropy::MessagesRequest) -> Result<ModelResponse> {
        let mr = ModelResponse {
            patch: None,
            operations: vec![],
            comment: None,
            usage: None,
            raw_response: None,
        };
        if let Some(message) = &req.messages.last() {
            if message.role == Role::Assistant {
                if message.content.is_empty() {
                    // We are seeing this happen fairly frequently with the Anthropic API.
                    return Err(TenxError::Throttle(Throttle::Backoff));
                }
                for content in &message.content {
                    if let Content::ToolUse(tool_use) = content {
                        match serde_json::from_value::<tools::TextEditor>(tool_use.input.clone()) {
                            Ok(_ed) => {
                                // Extract operations here
                            }
                            Err(e) => {
                                return Err(TenxError::Internal(format!(
                                    "Failed to parse tool use: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
            }
        }
        // TODO: Error if we don't have a patch
        // Err(TenxError::Internal("No patch to parse.".into()))
        Ok(mr)
    }

    fn request(
        &self,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
    ) -> Result<misanthropy::MessagesRequest> {
        let mut req = misanthropy::MessagesRequest {
            model: self.api_model.clone(),
            max_tokens: MAX_TOKENS,
            messages: Vec::new(),
            system: vec![],
            temperature: None,
            stream: true,
            tools: vec![],
            tool_choice: misanthropy::ToolChoice::Auto,
            stop_sequences: vec![],
        }
        .with_text_editor(misanthropy::TEXT_EDITOR_37);
        build_conversation(self, &mut req, config, session, dialect)?;
        Ok(req)
    }
}

impl Conversation<misanthropy::MessagesRequest> for ClaudeEditor {
    fn set_system_prompt(
        &self,
        req: &mut misanthropy::MessagesRequest,
        prompt: &str,
    ) -> Result<()> {
        req.system = vec![misanthropy::Content::Text(misanthropy::Text {
            text: prompt.into(),
            cache_control: Some(misanthropy::CacheControl::Ephemeral),
        })];
        Ok(())
    }

    fn add_user_message(&self, req: &mut misanthropy::MessagesRequest, text: &str) -> Result<()> {
        req.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::text(text)],
        });
        Ok(())
    }

    fn add_agent_message(&self, req: &mut misanthropy::MessagesRequest, text: &str) -> Result<()> {
        req.messages.push(misanthropy::Message {
            role: misanthropy::Role::Assistant,
            content: vec![misanthropy::Content::text(text)],
        });
        Ok(())
    }
}

#[async_trait::async_trait]
impl ModelProvider for ClaudeEditor {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn api_model(&self) -> String {
        self.api_model.clone()
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<EventSender>,
    ) -> Result<ModelResponse> {
        if self.anthropic_key.is_empty() {
            return Err(TenxError::Model(
                "No Anthropic key configured for Claude model.".into(),
            ));
        }

        if !session.should_continue() {
            return Err(TenxError::Internal("No prompt to process.".into()));
        }
        let dialect = config.dialect()?;
        let mut req = self.request(config, session, &dialect)?;
        req.stream = self.streaming;
        trace!("Sending request: {}", serde_json::to_string_pretty(&req)?);

        let resp = if self.streaming {
            self.stream_response(self.anthropic_key.clone(), &req, sender)
                .await?
        } else {
            let anthropic = Anthropic::new(&self.anthropic_key);
            let resp = anthropic.messages(&req).await?;
            if let Some(text) = resp.format_content().into() {
                send_event(&sender, Event::ModelResponse(text))?;
            }
            resp
        };

        trace!("Got response: {}", serde_json::to_string_pretty(&resp)?);

        req.merge_response(&resp);
        let mut modresp = self.extract_changes(&req)?;
        modresp.usage = Some(super::Usage::Claude(ClaudeUsage {
            input_tokens: resp.usage.input_tokens,
            output_tokens: resp.usage.output_tokens,
            cache_creation_input_tokens: resp.usage.cache_creation_input_tokens,
            cache_read_input_tokens: resp.usage.cache_read_input_tokens,
        }));
        Ok(modresp)
    }

    fn render(&self, config: &Config, session: &Session) -> Result<String> {
        let dialect = config.dialect()?;
        let req = self.request(config, session, &dialect)?;
        let json = serde_json::to_string_pretty(&req)?;
        Ok(json)
    }
}
