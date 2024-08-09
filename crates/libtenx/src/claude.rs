use tracing::warn;

use misanthropy::{Anthropic, ContentBlockDelta, StreamEvent};

use crate::{dialect::Dialect, extract_operations, Operations, Prompt, Result};

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;

#[derive(Debug)]
pub struct Claude<D: Dialect, F>
where
    F: FnMut(&str) -> Result<()>,
{
    api_key: String,
    conversation: misanthropy::MessagesRequest,
    dialect: D,
    on_chunk: F,
}

impl<D: Dialect, F> Claude<D, F>
where
    F: FnMut(&str) -> Result<()>,
{
    /// Creates a new Claude instance with the given API key, dialect, and chunk handler.
    pub fn new(api_key: &str, dialect: D, on_chunk: F) -> Result<Self> {
        let system = dialect.system();
        Ok(Claude {
            api_key: api_key.to_string(),
            dialect,
            on_chunk,
            conversation: misanthropy::MessagesRequest {
                model: DEFAULT_MODEL.to_string(),
                max_tokens: MAX_TOKENS,
                messages: vec![],
                system: Some(system),
                temperature: None,
                stream: true,
                tools: vec![],
                tool_choice: misanthropy::ToolChoice::Auto,
                stop_sequences: vec![],
            },
        })
    }

    /// Sends a prompt to Claude and returns the operations.
    pub async fn prompt(&mut self, prompt: &Prompt) -> Result<Operations> {
        let txt = self.dialect.render(prompt)?;
        self.conversation.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text {
                text: txt.to_string(),
            }],
        });
        let resp = self.stream_response().await?;
        self.conversation.merge_response(&resp);
        extract_operations(&self.conversation)
    }

    async fn stream_response(&mut self) -> Result<misanthropy::MessagesResponse> {
        let anthropic = Anthropic::new(&self.api_key);
        let mut streamed_response = anthropic.messages_stream(&self.conversation)?;
        while let Some(event) = streamed_response.next().await {
            let event = event?;
            match event {
                StreamEvent::ContentBlockDelta { delta, .. } => {
                    if let ContentBlockDelta::TextDelta { text } = delta {
                        (self.on_chunk)(&text)?;
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
}
