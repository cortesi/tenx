use tracing::warn;

use misanthropy::{Anthropic, ContentBlockDelta, StreamEvent};

use crate::{dialect::Dialect, extract_operations, Operations, Prompt, Result, Tenx};

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;

#[derive(Debug)]
pub struct Claude<F>
where
    F: FnMut(&str) -> Result<()>,
{
    conversation: misanthropy::MessagesRequest,
    on_chunk: F,
}

impl<F> Claude<F>
where
    F: FnMut(&str) -> Result<()>,
{
    /// Creates a new Claude instance with the given chunk handler.
    pub fn new(on_chunk: F) -> Result<Self> {
        Ok(Claude {
            on_chunk,
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
        })
    }

    /// Sends a prompt to Claude and returns the operations.
    pub async fn prompt(&mut self, tenx: &Tenx, prompt: &Prompt) -> Result<Operations> {
        self.conversation.system = Some(tenx.state.dialect.system());
        let txt = tenx.state.dialect.render(prompt)?;
        self.conversation.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text {
                text: txt.to_string(),
            }],
        });
        let resp = self.stream_response(&tenx.anthropic_key).await?;
        self.conversation.merge_response(&resp);
        extract_operations(&self.conversation)
    }

    async fn stream_response(&mut self, api_key: &str) -> Result<misanthropy::MessagesResponse> {
        let anthropic = Anthropic::new(api_key);
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

