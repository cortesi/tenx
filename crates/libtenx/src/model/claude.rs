//! This module implements the Claude model provider for the tenx system.
use std::{collections::HashMap, convert::From};

use misanthropy::{Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde::{Deserialize, Serialize};
use serde_json;
use tracing::{trace, warn};

use crate::{
    error::{Result, TenxError},
    events::*,
    model::tags,
    model::ModelProvider,
    session::ModelResponse,
    throttle::Throttle,
};

use super::Chat;

const MAX_TOKENS: u32 = 8192;

/// A model that interacts with the Anthropic API. The general design of the model is to:
///
/// - Have a large, cached system prompt with many examples.
/// - Emit both the non-editable context and the editable context as pre-primed messages in the
///   prompt.
/// - Edit the conversation to keep the most up-to-date editable files frontmost.
#[derive(Debug, Clone)]
pub struct ClaudeChat {
    /// Upstream model name to use
    pub api_model: String,
    /// The Anthropic API key
    pub anthropic_key: String,
    /// Whether to stream responses
    pub streaming: bool,
    /// The messages request being built
    request: misanthropy::MessagesRequest,
}

impl ClaudeChat {
    /// Creates a new ClaudeChat configured for the given model, API key, and streaming setting.
    pub fn new(api_model: String, anthropic_key: String, streaming: bool) -> Self {
        let request = misanthropy::MessagesRequest {
            model: api_model.clone(),
            max_tokens: MAX_TOKENS,
            messages: Vec::new(),
            system: vec![misanthropy::Content::Text(misanthropy::Text {
                text: tags::SYSTEM.into(),
                cache_control: Some(misanthropy::CacheControl::Ephemeral),
            })],
            temperature: None,
            stream: streaming,
            tools: vec![],
            tool_choice: misanthropy::ToolChoice::Auto,
            stop_sequences: vec![],
        };
        Self {
            api_model,
            anthropic_key,
            streaming,
            request,
        }
    }
    async fn stream_response(
        &self,
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
        if let Some(message) = &req.messages.last() {
            if message.role == Role::Assistant {
                if message.content.is_empty() {
                    // We are seeing this happen fairly frequently with the Anthropic API.
                    return Err(TenxError::Throttle(Throttle::Backoff));
                }
                for content in &message.content {
                    if let Content::Text(text) = content {
                        return tags::parse(&text.text);
                    }
                }
            }
        }
        Err(TenxError::Internal("No patch to parse.".into()))
    }

    fn append_last_message(&mut self, data: &str) -> Result<()> {
        // If there is no current message, create one
        if self.request.messages.is_empty()
            || self.request.messages.last().unwrap().role != Role::User
        {
            self.request.messages.push(misanthropy::Message {
                role: misanthropy::Role::User,
                content: vec![misanthropy::Content::text(data)],
            });
        } else {
            // Get the last message
            let last_message = self.request.messages.last_mut().unwrap();
            // Append to content - assumes the last content block is Text
            if let Some(misanthropy::Content::Text(text)) = last_message.content.last_mut() {
                text.text.push_str(data);
            } else {
                // If the last content block isn't text, add a new one
                last_message.content.push(misanthropy::Content::text(data));
            }
        }
        Ok(())
    }
}

impl ClaudeChat {
    /// Helper to add or append a message with the given role.
    fn add_message_with_role(&mut self, role: misanthropy::Role, text: &str) -> Result<()> {
        if self.request.messages.is_empty()
            || self.request.messages.last().unwrap().role != role
        {
            self.request.messages.push(misanthropy::Message {
                role,
                content: vec![misanthropy::Content::text(text)],
            });
        } else {
            let last_message = self.request.messages.last_mut().unwrap();
            if let Some(misanthropy::Content::Text(text_content)) = last_message.content.last_mut() {
                text_content.text.push_str(text);
            } else {
                last_message.content.push(misanthropy::Content::text(text));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Chat for ClaudeChat {
    fn add_system_prompt(&mut self, prompt: &str) -> Result<()> {
        self.request
            .system
            .push(misanthropy::Content::Text(misanthropy::Text {
                text: prompt.into(),
                cache_control: Some(misanthropy::CacheControl::Ephemeral),
            }));
        Ok(())
    }

    fn add_user_message(&mut self, text: &str) -> Result<()> {
        self.add_message_with_role(misanthropy::Role::User, text)
    }

    fn add_agent_message(&mut self, text: &str) -> Result<()> {
        self.add_message_with_role(misanthropy::Role::Assistant, text)
    }

    fn add_context(&mut self, _name: &str, data: &str) -> Result<()> {
        self.append_last_message(data)
    }

    fn add_editable(&mut self, _path: &str, data: &str) -> Result<()> {
        self.append_last_message(data)
    }

    async fn send(&mut self, sender: Option<EventSender>) -> Result<ModelResponse> {
        if self.anthropic_key.is_empty() {
            return Err(TenxError::Model(
                "No Anthropic key configured for Claude model.".into(),
            ));
        }

        self.request.model = self.api_model.clone();
        self.request.max_tokens = MAX_TOKENS;
        self.request.stream = self.streaming;

        trace!(
            "Sending request: {}",
            serde_json::to_string_pretty(&self.request)?
        );

        let resp = if self.streaming {
            self.stream_response(self.anthropic_key.clone(), &self.request, sender.clone())
                .await?
        } else {
            let anthropic = Anthropic::new(&self.anthropic_key);
            let resp = anthropic.messages(&self.request).await?;
            if let Some(text) = resp.format_content().into() {
                send_event(&sender, Event::ModelResponse(text))?;
            }
            resp
        };

        trace!("Got response: {}", serde_json::to_string_pretty(&resp)?);

        self.request.merge_response(&resp);

        let mut modresp = self.extract_changes(&self.request)?;
        modresp.usage = Some(super::Usage::Claude(ClaudeUsage {
            input_tokens: resp.usage.input_tokens,
            output_tokens: resp.usage.output_tokens,
            cache_creation_input_tokens: resp.usage.cache_creation_input_tokens,
            cache_read_input_tokens: resp.usage.cache_read_input_tokens,
        }));
        Ok(modresp)
    }

    fn render(&self) -> Result<String> {
        let json = serde_json::to_string_pretty(&self.request)?;
        Ok(json)
    }
}

/// A model that interacts with the Anthropic API. The general design of the model is to:
///
/// - Have a large, cached system prompt with many examples.
/// - Emit both the non-editable context and the editable context as pre-primed messages in the
///   prompt.
/// - Edit the conversation to keep the most up-to-date editable files frontmost.
#[derive(Default, Debug, Clone)]
pub struct Claude {
    /// The user facing name of the model
    pub name: String,
    /// Upstream model name to use
    pub api_model: String,
    /// The Anthropic API key
    pub anthropic_key: String,
    /// Whether to stream responses
    pub streaming: bool,
}

/// Mirrors the Usage struct from misanthropy to track token usage statistics.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct ClaudeUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}

impl ClaudeUsage {
    pub fn values(&self) -> HashMap<String, u64> {
        let mut map = HashMap::new();
        if let Some(input_tokens) = self.input_tokens {
            map.insert("input_tokens".to_string(), input_tokens as u64);
        }
        if let Some(output_tokens) = self.output_tokens {
            map.insert("output_tokens".to_string(), output_tokens as u64);
        }
        if let Some(cache_creation_input_tokens) = self.cache_creation_input_tokens {
            map.insert(
                "cache_creation_input_tokens".to_string(),
                cache_creation_input_tokens as u64,
            );
        }
        if let Some(cache_read_input_tokens) = self.cache_read_input_tokens {
            map.insert(
                "cache_read_input_tokens".to_string(),
                cache_read_input_tokens as u64,
            );
        }
        map
    }

    pub fn totals(&self) -> (u64, u64) {
        let input = self.input_tokens.unwrap_or(0) as u64
            + self.cache_creation_input_tokens.unwrap_or(0) as u64
            + self.cache_read_input_tokens.unwrap_or(0) as u64;
        let output = self.output_tokens.unwrap_or(0) as u64;
        (input, output)
    }
}

impl From<serde_json::Error> for TenxError {
    fn from(error: serde_json::Error) -> Self {
        TenxError::Internal(error.to_string())
    }
}

#[async_trait::async_trait]
impl ModelProvider for Claude {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn chat(&self) -> Option<Box<dyn Chat>> {
        Some(Box::new(ClaudeChat::new(
            self.api_model.clone(),
            self.anthropic_key.clone(),
            self.streaming,
        )))
    }

    fn api_model(&self) -> String {
        self.api_model.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of_first_content(message: &misanthropy::Message) -> &str {
        if let Content::Text(text_content) = &message.content[0] {
            &text_content.text
        } else {
            panic!("Expected Text content");
        }
    }

    #[test]
    fn test_add_user_and_agent_message() {
        let mut chat = ClaudeChat::new(
            "claude-3-opus-20240229".to_string(),
            "fake-key".to_string(),
            false,
        );

        // Add user message and check
        chat.add_user_message("Hello").unwrap();
        assert_eq!(chat.request.messages.len(), 1);
        assert_eq!(chat.request.messages[0].role, Role::User);
        assert_eq!(text_of_first_content(&chat.request.messages[0]), "Hello");

        // Append to user message
        chat.add_user_message(" World").unwrap();
        assert_eq!(chat.request.messages.len(), 1);
        assert_eq!(text_of_first_content(&chat.request.messages[0]), "Hello World");

        // Add agent message and check
        chat.add_agent_message("I'm Claude").unwrap();
        assert_eq!(chat.request.messages.len(), 2);
        assert_eq!(chat.request.messages[1].role, Role::Assistant);
        assert_eq!(text_of_first_content(&chat.request.messages[1]), "I'm Claude");

        // Add another user message (should create new)
        chat.add_user_message("Nice to meet you").unwrap();
        assert_eq!(chat.request.messages.len(), 3);
        assert_eq!(chat.request.messages[2].role, Role::User);
        assert_eq!(text_of_first_content(&chat.request.messages[2]), "Nice to meet you");

        // Add another agent message (should create new)
        chat.add_agent_message("How can I help?").unwrap();
        assert_eq!(chat.request.messages.len(), 4);
        assert_eq!(chat.request.messages[3].role, Role::Assistant);
        assert_eq!(text_of_first_content(&chat.request.messages[3]), "How can I help?");
    }
}
