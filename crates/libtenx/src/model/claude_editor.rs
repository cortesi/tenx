//! This module implements the Claude model provider with text editor capabilities for the tenx system.
use misanthropy::{tools, Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde_json;
use tracing::{trace, warn};

use super::claude::ClaudeUsage;
use crate::{
    error::{Result, TenxError},
    events::*,
    model::ModelProvider,
    session::ModelResponse,
    throttle::Throttle,
};

use super::Chat;
use state;

const MAX_TOKENS: u32 = 8192;

/// A chat implementation for Claude with text editor capabilities
#[derive(Debug, Clone)]
pub struct ClaudeEditorChat {
    /// Upstream model name to use
    pub api_model: String,
    /// The Anthropic API key
    pub anthropic_key: String,
    /// Whether to stream responses
    pub streaming: bool,
    /// The messages request being built
    request: misanthropy::MessagesRequest,
}

impl ClaudeEditorChat {
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
        let last_message = match req.messages.last() {
            Some(message) if message.role == Role::Assistant => message,
            _ => {
                return Ok(ModelResponse::default());
            }
        };

        if last_message.content.is_empty() {
            // We are seeing this happen fairly frequently with the Anthropic API.
            return Err(TenxError::Throttle(Throttle::Backoff));
        }

        // Extract comment from text content
        let mut comment = None;
        for content in &last_message.content {
            if let Content::Text(text) = content {
                comment = Some(text.text.clone());
                break;
            }
        }

        // Process tool uses
        let mut patch = state::Patch::default();

        for content in &last_message.content {
            if let Content::ToolUse(tool_use) = content {
                match serde_json::from_value::<tools::TextEditor>(tool_use.input.clone()) {
                    Ok(edit) => match edit {
                        tools::TextEditor::Create { path, file_text } => {
                            patch = patch.with_write(path, file_text);
                        }
                        tools::TextEditor::StrReplace {
                            path,
                            old_str,
                            new_str,
                        } => {
                            patch = patch.with_replace(path, old_str, new_str);
                        }
                        tools::TextEditor::Insert {
                            path,
                            insert_line,
                            new_str,
                        } => {
                            patch = patch.with_insert(path, insert_line, new_str);
                        }
                        tools::TextEditor::View { path, view_range } => {
                            if let Some(range) = view_range {
                                patch = patch.with_view_range_onebased(
                                    path,
                                    range[0] as isize,
                                    range[1] as isize,
                                );
                            } else {
                                patch = patch.with_view(path);
                            }
                        }
                        tools::TextEditor::UndoEdit { path } => {
                            patch = patch.with_undo(path);
                        }
                    },
                    Err(e) => {
                        return Err(TenxError::Internal(format!(
                            "Failed to parse tool use: {}",
                            e
                        )));
                    }
                }
            }
        }

        // Create the ModelResponse at the end
        Ok(ModelResponse {
            patch: if !patch.is_empty() { Some(patch) } else { None },
            operations: vec![],
            comment,
            usage: None,
            raw_response: Some(last_message.format_content()),
        })
    }
}

#[async_trait::async_trait]
impl Chat for ClaudeEditorChat {
    fn add_system_prompt(&mut self, prompt: &str) -> Result<()> {
        self.request.system = vec![misanthropy::Content::Text(misanthropy::Text {
            text: prompt.into(),
            cache_control: Some(misanthropy::CacheControl::Ephemeral),
        })];
        Ok(())
    }

    fn add_user_message(&mut self, text: &str) -> Result<()> {
        self.request.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::text(text)],
        });
        Ok(())
    }

    fn add_agent_message(&mut self, text: &str) -> Result<()> {
        self.request.messages.push(misanthropy::Message {
            role: misanthropy::Role::Assistant,
            content: vec![misanthropy::Content::text(text)],
        });
        Ok(())
    }

    fn add_context(&mut self, name: &str, data: &str) -> Result<()> {
        // Add context as a user message with a clear marker
        self.add_user_message(&format!("<context name=\"{}\">{}\\</context>", name, data))
    }

    fn add_editable(&mut self, path: &str, data: &str) -> Result<()> {
        // Add editable content as a user message with a clear marker
        self.add_user_message(&format!(
            "<editable path=\"{}\">{}\\</editable>",
            path, data
        ))
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

#[async_trait::async_trait]
impl ModelProvider for ClaudeEditor {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn api_model(&self) -> String {
        self.api_model.clone()
    }

    fn chat(&self) -> Option<Box<dyn Chat>> {
        let mut request = misanthropy::MessagesRequest {
            model: self.api_model.clone(),
            max_tokens: MAX_TOKENS,
            messages: Vec::new(),
            system: vec![],
            temperature: None,
            stream: self.streaming,
            tools: vec![],
            tool_choice: misanthropy::ToolChoice::Auto,
            stop_sequences: vec![],
        };

        // Add text editor tool
        request = request.with_text_editor(misanthropy::TEXT_EDITOR_37);

        Some(Box::new(ClaudeEditorChat {
            api_model: self.api_model.clone(),
            anthropic_key: self.anthropic_key.clone(),
            streaming: self.streaming,
            request,
        }))
    }
}
