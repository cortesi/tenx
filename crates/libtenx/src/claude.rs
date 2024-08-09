use misanthropy::{Anthropic, ContentBlockDelta, StreamEvent};

use crate::{dialect::Dialect, extract_operations, Operations, Prompt, Result};

const SYSTEM: &str = include_str!("../prompts/claude_system.txt");
const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;

#[derive(Debug)]
pub struct Claude<D: Dialect> {
    anthropic: Anthropic,
    conversation: misanthropy::MessagesRequest,
    dialect: D,
}

impl<D: Dialect> Claude<D> {
    pub fn new(api_key: &str, dialect: D) -> Result<Self> {
        let anthropic = Anthropic::from_string_or_env(api_key)?;
        Ok(Claude {
            anthropic,
            dialect,
            conversation: misanthropy::MessagesRequest {
                model: DEFAULT_MODEL.to_string(),
                max_tokens: MAX_TOKENS,
                messages: vec![],
                system: Some(SYSTEM.to_string()),
                temperature: None,
                stream: true,
                tools: vec![],
                tool_choice: misanthropy::ToolChoice::Auto,
                stop_sequences: vec![],
            },
        })
    }

    fn add_prompt(&mut self, prompt: &Prompt) -> Result<()> {
        let txt = self.dialect.render(prompt)?;
        self.conversation.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text {
                text: txt.to_string(),
            }],
        });
        Ok(())
    }

    pub async fn prompt<F>(&mut self, prompt: &Prompt, progress: F) -> Result<Operations>
    where
        F: FnMut(&str) -> Result<()>,
    {
        self.add_prompt(prompt)?;
        let resp = self.stream_response(&self.conversation, progress).await?;
        self.conversation.merge_response(&resp);
        extract_operations(&self.conversation)
    }

    pub async fn stream_response<F>(
        &self,
        request: &misanthropy::MessagesRequest,
        mut on_chunk: F,
    ) -> Result<misanthropy::MessagesResponse>
    where
        F: FnMut(&str) -> Result<()>,
    {
        let mut streamed_response = self.anthropic.messages_stream(request)?;
        while let Some(event) = streamed_response.next().await {
            let event = event?;
            match event {
                StreamEvent::ContentBlockDelta { delta, .. } => {
                    if let ContentBlockDelta::TextDelta { text } = delta {
                        on_chunk(&text)?;
                    }
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
