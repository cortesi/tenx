//! This module implements the Claude model provider for the tenx system.
use tracing::warn;

use misanthropy::{Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde::{Deserialize, Serialize};
use serde_json;

use super::ModelProvider;
use crate::{
    dialect::{Dialect, DialectProvider},
    events::Event,
    patch, Config, Result, Session, TenxError,
};
use std::collections::HashSet;
use std::convert::From;
use std::path::PathBuf;

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;
const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.\n";
const EDITABLE_LEADIN: &str =
    "Here are the editable files. You will modify only these, nothing else.\n";
const EDITABLE_UPDATE_LEADIN: &str = "Here are the updated files.";
const OMITTED_FILES_LEADIN: &str =
    "These files have been omitted since they were updated later in the conversation:";

use tokio::sync::mpsc;

/// A model that interacts with the Anthropic API. This general design of the model is to:
///
/// - Have a large, cached system prompt with many examples.
/// - Emit both the non-editable context and the editable context as pre-primed messages in the
///   prompt.
/// - Edit the conversation to keep the most up-to-date editable files frontmost.
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Claude {}

impl From<serde_json::Error> for TenxError {
    fn from(error: serde_json::Error) -> Self {
        TenxError::Internal(error.to_string())
    }
}

impl Claude {
    async fn stream_response(
        &mut self,
        api_key: &str,
        req: &misanthropy::MessagesRequest,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<misanthropy::MessagesResponse> {
        let anthropic = Anthropic::new(api_key);
        let mut streamed_response = anthropic.messages_stream(req)?;
        while let Some(event) = streamed_response.next().await {
            let event = event?;
            match event {
                StreamEvent::ContentBlockDelta { delta, .. } => {
                    if let ContentBlockDelta::TextDelta { text } = delta {
                        if let Some(sender) = &sender {
                            if let Err(e) = sender.send(Event::Snippet(text)).await {
                                warn!("Error sending message to channel: {:?}", e);
                            }
                        }
                    }
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
    ) -> Result<patch::Patch> {
        if let Some(message) = &req.messages.last() {
            if message.role == Role::Assistant {
                for content in &message.content {
                    if let Content::Text { text } = content {
                        return dialect.parse(text);
                    }
                }
            }
        }
        Err(TenxError::Internal("No patch to parse.".into()))
    }

    /// Filters the given files based on whether they will be modified in future steps.
    ///
    /// Returns a tuple of (included, omitted) files.
    fn filter_files(
        &self,
        files: &[PathBuf],
        session: &Session,
        step_offset: usize,
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut future_modified_files = HashSet::new();
        for step in session.steps().iter().skip(step_offset + 1) {
            if let Some(patch) = &step.patch {
                future_modified_files.extend(patch.changed_files());
            }
        }
        let (included, omitted): (Vec<_>, Vec<_>) = files
            .iter()
            .partition(|file| !future_modified_files.contains(*file));
        (
            included.into_iter().cloned().collect(),
            omitted.into_iter().cloned().collect(),
        )
    }

    /// Renders the editable files and appends a list of omitted files if any.
    ///
    /// Returns a formatted string containing the rendered editables and omitted files.
    fn render_editables_with_omitted(
        &self,
        session: &Session,
        files: Vec<PathBuf>,
        omitted: Vec<PathBuf>,
    ) -> Result<String> {
        let mut result = session.dialect.render_editables(files)?;
        if !omitted.is_empty() {
            result.push_str(&format!("\n{}\n", OMITTED_FILES_LEADIN));
            for file in omitted {
                result.push_str(&format!("- {}\n", file.display()));
            }
        }
        Ok(result)
    }

    fn request(&self, session: &Session) -> Result<misanthropy::MessagesRequest> {
        let mut req = misanthropy::MessagesRequest {
            model: DEFAULT_MODEL.to_string(),
            max_tokens: MAX_TOKENS,
            messages: vec![
                misanthropy::Message {
                    role: misanthropy::Role::User,
                    content: vec![misanthropy::Content::Text {
                        text: format!(
                            "{}\n{}",
                            CONTEXT_LEADIN,
                            session.dialect.render_context(session)?
                        ),
                    }],
                },
                misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::Text {
                        text: "Got it.".to_string(),
                    }],
                },
                misanthropy::Message {
                    role: misanthropy::Role::User,
                    content: vec![misanthropy::Content::Text {
                        text: format!("{}\n{}", EDITABLE_LEADIN, {
                            let (included, omitted) =
                                self.filter_files(&session.editables()?, session, 0);
                            self.render_editables_with_omitted(session, included, omitted)?
                        }),
                    }],
                },
                misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::Text {
                        text: "Got it.".to_string(),
                    }],
                },
            ],
            system: Some(session.dialect.system()),
            temperature: None,
            stream: true,
            tools: vec![],
            tool_choice: misanthropy::ToolChoice::Auto,
            stop_sequences: vec![],
        };
        for (i, s) in session.steps().iter().enumerate() {
            req.messages.push(misanthropy::Message {
                role: misanthropy::Role::User,
                content: vec![misanthropy::Content::Text {
                    text: session.dialect.render_step_request(session, i)?,
                }],
            });
            if let Some(patch) = &s.patch {
                req.messages.push(misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::Text {
                        text: session.dialect.render_step_response(session, i)?,
                    }],
                });
                let (included, omitted) = self.filter_files(&patch.changed_files(), session, i);
                req.messages.push(misanthropy::Message {
                    role: misanthropy::Role::User,
                    content: vec![misanthropy::Content::Text {
                        text: format!(
                            "{}\n{}",
                            EDITABLE_UPDATE_LEADIN,
                            self.render_editables_with_omitted(session, included, omitted)?
                        ),
                    }],
                });
                req.messages.push(misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::Text {
                        text: "Got it.".to_string(),
                    }],
                });
            }
        }
        Ok(req)
    }
}

#[async_trait::async_trait]
impl ModelProvider for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<patch::Patch> {
        if !session.pending_prompt() {
            return Err(TenxError::Internal("No pending prompt to process.".into()));
        }
        let mut req = self.request(session)?;
        let resp = self
            .stream_response(&config.anthropic_key, &req, sender)
            .await?;
        req.merge_response(&resp);
        self.extract_changes(&session.dialect, &req)
    }

    fn render(&self, session: &Session) -> Result<String> {
        let req = self.request(session)?;
        let json = serde_json::to_string_pretty(&req)?;
        Ok(json)
    }
}
