use tracing::warn;

use misanthropy::{Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde::{Deserialize, Serialize};

use crate::{
    dialect::{Dialect, Dialects},
    operations, Config, Operations, Prompt, Result,
};

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;

use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claude {
    conversation: misanthropy::MessagesRequest,
}

impl Default for Claude {
    fn default() -> Self {
        Claude {
            conversation: misanthropy::MessagesRequest {
                model: DEFAULT_MODEL.to_string(),
                max_tokens: MAX_TOKENS,
                messages: vec![],
                system: None,
                temperature: None,
                stream: true,
                tools: vec![],
                tool_choice: misanthropy::ToolChoice::Auto,
                stop_sequences: vec![],
            },
        }
    }
}

impl Claude {
    /// Creates a new conversation, then sends a prompt to the active and returns the resulting
    /// operations.
    ///
    /// Takes a reference to Tenx, a Prompt, and an optional mpsc::Sender for streaming text
    /// chunks. Returns a Result containing the extracted Operations.
    pub async fn start(
        &mut self,
        config: &Config,
        dialect: &Dialects,
        prompt: &Prompt,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<Operations> {
        self.conversation.system = Some(dialect.system());
        let txt = dialect.render(prompt)?;
        self.conversation.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text {
                text: txt.to_string(),
            }],
        });
        let resp = self.stream_response(&config.anthropic_key, sender).await?;
        self.conversation.merge_response(&resp);
        self.extract_operations()
    }

    async fn stream_response(
        &mut self,
        api_key: &str,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<misanthropy::MessagesResponse> {
        let anthropic = Anthropic::new(api_key);
        let mut streamed_response = anthropic.messages_stream(&self.conversation)?;
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

    fn extract_operations(&self) -> Result<Operations> {
        let mut operations = Operations::default();
        for message in &self.conversation.messages {
            if message.role == Role::Assistant {
                for content in &message.content {
                    if let Content::Text { text } = content {
                        let parsed_ops = operations::parse_response_text(text)?;
                        operations.operations.extend(parsed_ops.operations);
                    }
                }
            }
        }
        Ok(operations)
    }
}
