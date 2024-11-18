//! This module implements the Claude model provider for the tenx system.
use std::{collections::HashMap, convert::From, path::PathBuf};

use misanthropy::{Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde::{Deserialize, Serialize};
use serde_json;
use tokio::sync::mpsc;
use tracing::{trace, warn};

use crate::{
    config::Config,
    dialect::{Dialect, DialectProvider},
    events::*,
    model::ModelProvider,
    session::ModelResponse,
    Result, Session, TenxError,
};

const MAX_TOKENS: u32 = 8192;
const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.\n";
const EDITABLE_LEADIN: &str =
    "Here are the editable files. You will modify only these, nothing else.\n";
const EDITABLE_UPDATE_LEADIN: &str = "Here are the updated files.";
const OMITTED_FILES_LEADIN: &str =
    "These files have been omitted since they were updated later in the conversation:";

/// A model that interacts with the Anthropic API. The general design of the model is to:
///
/// - Have a large, cached system prompt with many examples.
/// - Emit both the non-editable context and the editable context as pre-primed messages in the
///   prompt.
/// - Edit the conversation to keep the most up-to-date editable files frontmost.
#[derive(Default, Debug, Clone)]
pub struct Claude {
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

impl Claude {
    async fn stream_response(
        &mut self,
        api_key: String,
        req: &misanthropy::MessagesRequest,
        sender: Option<mpsc::Sender<Event>>,
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

    fn extract_changes(
        &self,
        dialect: &Dialect,
        req: &misanthropy::MessagesRequest,
    ) -> Result<ModelResponse> {
        if let Some(message) = &req.messages.last() {
            if message.role == Role::Assistant {
                for content in &message.content {
                    if let Content::Text(text) = content {
                        return dialect.parse(&text.text);
                    }
                }
            }
        }
        Err(TenxError::Internal("No patch to parse.".into()))
    }

    /// Renders the editable files and appends a list of omitted files if any.
    ///
    /// Returns a formatted string containing the rendered editables and omitted files.
    fn render_editables_with_omitted(
        &self,
        config: &Config,
        session: &Session,
        dialect: &Dialect,
        files: Vec<PathBuf>,
        omitted: Vec<PathBuf>,
    ) -> Result<String> {
        let mut result = dialect.render_editables(config, session, files)?;
        if !omitted.is_empty() {
            result.push_str(&format!("\n{}\n", OMITTED_FILES_LEADIN));
            for file in omitted {
                result.push_str(&format!("- {}\n", file.display()));
            }
        }
        Ok(result)
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
            messages: vec![
                misanthropy::Message {
                    role: misanthropy::Role::User,
                    content: vec![misanthropy::Content::Text(misanthropy::Text {
                        text: format!(
                            "{}\n{}",
                            CONTEXT_LEADIN,
                            dialect.render_context(config, session)?
                        ),
                        cache_control: Some(misanthropy::CacheControl::Ephemeral),
                    })],
                },
                misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::text("Got it")],
                },
                misanthropy::Message {
                    role: misanthropy::Role::User,
                    content: vec![misanthropy::Content::text(format!(
                        "{}\n{}",
                        EDITABLE_LEADIN,
                        {
                            let (included, omitted) =
                                session.partition_modified(session.editable(), 0);
                            self.render_editables_with_omitted(
                                config, session, dialect, included, omitted,
                            )?
                        }
                    ))],
                },
                misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::text("Got it")],
                },
            ],
            system: vec![misanthropy::Content::Text(misanthropy::Text {
                text: dialect.system(),
                cache_control: Some(misanthropy::CacheControl::Ephemeral),
            })],
            temperature: None,
            stream: true,
            tools: vec![],
            tool_choice: misanthropy::ToolChoice::Auto,
            stop_sequences: vec![],
        };
        for (i, s) in session.steps().iter().enumerate() {
            req.messages.push(misanthropy::Message {
                role: misanthropy::Role::User,
                content: vec![misanthropy::Content::text(
                    dialect.render_step_request(config, session, i)?,
                )],
            });
            if let Some(resp) = &s.model_response {
                if let Some(patch) = &resp.patch {
                    req.messages.push(misanthropy::Message {
                        role: misanthropy::Role::Assistant,
                        content: vec![misanthropy::Content::text(
                            dialect.render_step_response(config, session, i)?,
                        )],
                    });
                    let (included, omitted) = session.partition_modified(&patch.changed_files(), i);
                    req.messages.push(misanthropy::Message {
                        role: misanthropy::Role::User,
                        content: vec![misanthropy::Content::text(format!(
                            "{}\n{}",
                            EDITABLE_UPDATE_LEADIN,
                            self.render_editables_with_omitted(
                                config, session, dialect, included, omitted
                            )?
                        ))],
                    });
                    req.messages.push(misanthropy::Message {
                        role: misanthropy::Role::Assistant,
                        content: vec![misanthropy::Content::text("Got it.")],
                    });
                }
            } else if i != session.steps().len() - 1 {
                req.messages.push(misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::text("omitted due to error")],
                });
            }
        }
        Ok(req)
    }
}

#[async_trait::async_trait]
impl ModelProvider for Claude {
    fn name(&self) -> String {
        self.api_model.clone()
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
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
        let mut modresp = self.extract_changes(&dialect, &req)?;
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
