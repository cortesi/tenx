use colored::*;
use tracing::warn;

use misanthropy::{Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde::{Deserialize, Serialize};

use super::ModelProvider;
use crate::{
    dialect::{Dialect, DialectProvider},
    patch, Config, Result, Session, TenxError,
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
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claude {
    conversation: misanthropy::MessagesRequest,
}

impl Claude {
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

    fn extract_changes(&self, dialect: &Dialect) -> Result<patch::Patch> {
        let mut cset = patch::Patch::default();
        for message in &self.conversation.messages {
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

    /// Updates the context messages in the conversation.
    ///
    /// This method handles several scenarios:
    /// - If the context is empty, it removes any existing context messages.
    /// - If the conversation is empty, it appends the new context messages.
    /// - If the conversation has existing context messages, it replaces them.
    /// - If the conversation has messages but no context, it inserts the context at the start.
    ///
    /// # Arguments
    ///
    /// * `session` - The current session.
    /// * `dialect` - The dialect used for rendering context.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the operation was successful, or an error if context rendering fails.
    fn update_context_messages(&mut self, session: &Session) -> Result<()> {
        let preamble = vec![
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
        ];
        if self.conversation.messages.is_empty() {
            // Append context messages if conversation is empty
            self.conversation.messages.extend(preamble);
        } else {
            // Replace leading messages - we can assume that the conversation has at least the
            // preamble length.
            let mut i = 0;
            while i < preamble.len() {
                self.conversation.messages[i] = preamble[i].clone();
                i += 1;
            }
        }

        Ok(())
    }
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

#[async_trait::async_trait]
impl ModelProvider for Claude {
    fn pretty_print(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("{}\n", "Claude Model Conversation".bold().green()));
        output.push_str(&format!("{}\n", "=========================".green()));

        for (i, message) in self.conversation.messages.iter().enumerate() {
            let role = match message.role {
                Role::User => "User".bold().yellow(),
                Role::Assistant => "Assistant".bold().cyan(),
            };
            output.push_str(&format!("{}. {}:\n", i + 1, role));
            for content in &message.content {
                if let Content::Text { text } = content {
                    output.push_str(&format!("{}\n\n", text));
                }
            }
        }
        output
    }

    async fn prompt(
        &mut self,
        config: &Config,
        dialect: &Dialect,
        session: &Session,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<patch::Patch> {
        self.conversation.system = Some(dialect.system());
        let prompt = session
            .steps
            .last()
            .ok_or(TenxError::Internal("no steps".into()))?
            .prompt
            .clone();

        self.update_context_messages(session)?;

        let txt = dialect.render_prompt(&prompt)?;
        self.conversation.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text { text: txt }],
        });

        let resp = self.stream_response(&config.anthropic_key, sender).await?;
        self.conversation.merge_response(&resp);
        self.extract_changes(dialect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialect::Dialect, Context};

    #[test]
    fn test_update_context_messages() -> Result<()> {
        let mut claude = Claude::default();
        let mut session = Session::new(
            Some(".".into()),
            Dialect::Tags(crate::dialect::Tags {}),
            crate::model::Model::Claude(claude.clone()),
        );

        // Test empty context
        claude.update_context_messages(&session)?;
        assert_eq!(claude.conversation.messages.len(), 4);

        // Test adding context to empty conversation
        session.add_context(Context {
            ty: crate::session::ContextType::File,
            name: "test".to_string(),
            data: crate::session::ContextData::String("Test context".to_string()),
        });
        claude.update_context_messages(&session)?;
        assert_eq!(claude.conversation.messages.len(), 4);
        if let misanthropy::Content::Text { text } = &claude.conversation.messages[0].content[0] {
            assert!(text.starts_with(CONTEXT_LEADIN));
        } else {
            panic!("Expected Text content");
        }

        // Test replacing existing context
        session.add_context(Context {
            ty: crate::session::ContextType::File,
            name: "test2".to_string(),
            data: crate::session::ContextData::String("New test context".to_string()),
        });
        claude.update_context_messages(&session)?;
        assert_eq!(claude.conversation.messages.len(), 4);
        if let misanthropy::Content::Text { text } = &claude.conversation.messages[0].content[0] {
            assert!(text.contains("New test context"));
        } else {
            panic!("Expected Text content");
        }

        // Test inserting context at start of non-empty conversation
        claude.conversation.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text {
                text: "User message".to_string(),
            }],
        });
        claude.update_context_messages(&session)?;

        assert_eq!(claude.conversation.messages.len(), 5);
        if let misanthropy::Content::Text { text } = &claude.conversation.messages[0].content[0] {
            assert!(text.starts_with(CONTEXT_LEADIN));
        } else {
            panic!("Expected Text content");
        }
        if let misanthropy::Content::Text { text } = &claude.conversation.messages[4].content[0] {
            assert_eq!(text, "User message");
        } else {
            panic!("Expected Text content");
        }

        Ok(())
    }
}
