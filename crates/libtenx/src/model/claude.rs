use tracing::warn;

use misanthropy::{Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde::{Deserialize, Serialize};

use super::ModelProvider;
use crate::{
    dialect::{Dialect, DialectProvider},
    patch, Config, Result, Session,
};

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;
const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.\n";
const EDITABLE_LEADIN: &str =
    "Here are the editable files. You will modify only these, nothing else.\n";

use tokio::sync::mpsc;

/// A model that interacts with the Anthropic API. This general design of the model is to:
///
/// - Have a large, cached system prompt with many examples.
/// - Emit both the non-editable context and the editable context as pre-primed messages in the
///   prompt.
/// - Edit the conversation to keep the most up-to-date editable files frontmost.
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Claude {}

impl Claude {
    async fn stream_response(
        &mut self,
        api_key: &str,
        req: &misanthropy::MessagesRequest,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<misanthropy::MessagesResponse> {
        let anthropic = Anthropic::new(api_key);
        let mut streamed_response = anthropic.messages_stream(req)?;
        while let Some(event) = streamed_response.next().await {
            let event = event?;
            match event {
                StreamEvent::ContentBlockDelta { delta, .. } => {
                    if let ContentBlockDelta::TextDelta { text } = delta {
                        if let Some(sender) = &sender {
                            if let Err(e) = sender.send(text).await {
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
        let mut cset = patch::Patch::default();
        for message in &req.messages {
            if message.role == Role::Assistant {
                for content in &message.content {
                    if let Content::Text { text } = content {
                        let parsed_ops = dialect.parse(text)?;
                        cset.changes.extend(parsed_ops.changes);
                    }
                }
            }
        }
        Ok(cset)
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
                        text: format!(
                            "{}\n{}",
                            EDITABLE_LEADIN,
                            session.dialect.render_editables(session.editable.clone())?
                        ),
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
        for s in &session.steps {
            req.messages.push(misanthropy::Message {
                role: misanthropy::Role::User,
                content: vec![misanthropy::Content::Text {
                    text: session.dialect.render_prompt(&s.prompt)?,
                }],
            });
            if let Some(patch) = &s.patch {
                req.messages.push(misanthropy::Message {
                    role: misanthropy::Role::Assistant,
                    content: vec![misanthropy::Content::Text {
                        text: session.dialect.render_patch(patch)?,
                    }],
                });
            }
        }
        Ok(req)
    }
}

#[async_trait::async_trait]
impl ModelProvider for Claude {
    async fn prompt(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<patch::Patch> {
        let mut req = self.request(session)?;
        println!("Request: {:#?}", req);
        let resp = self
            .stream_response(&config.anthropic_key, &req, sender)
            .await?;
        req.merge_response(&resp);
        self.extract_changes(&session.dialect, &req)
    }
}
