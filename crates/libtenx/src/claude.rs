use misanthropy::{Anthropic, ContentBlockDelta, StreamEvent};

use crate::{Context, Result, Workspace};

const SYSTEM: &str = include_str!("../prompts/claude_system.txt");
const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;

#[derive(Debug)]
pub struct Claude {
    anthropic: Anthropic,
}

impl Claude {
    pub fn new(api_key: &str) -> Result<Self> {
        let anthropic = Anthropic::from_string_or_env(api_key)?;
        Ok(Claude { anthropic })
    }

    pub async fn render(
        &self,
        ctx: &Context,
        workspace: &Workspace,
    ) -> Result<misanthropy::MessagesRequest> {
        let txt = ctx.render(workspace)?;

        Ok(misanthropy::MessagesRequest {
            model: DEFAULT_MODEL.to_string(),
            max_tokens: MAX_TOKENS,
            messages: vec![misanthropy::Message {
                role: misanthropy::Role::User,
                content: vec![misanthropy::Content::Text {
                    text: txt.to_string(),
                }],
            }],
            system: Some(SYSTEM.to_string()),
            temperature: None,
            stream: true,
            tools: vec![],
            tool_choice: misanthropy::ToolChoice::Auto,
            stop_sequences: vec![],
        })
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
