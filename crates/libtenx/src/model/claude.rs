use colored::*;
use tracing::warn;

use misanthropy::{Anthropic, Content, ContentBlockDelta, Role, StreamEvent};
use serde::{Deserialize, Serialize};

use super::ModelProvider;
use crate::{
    changes,
    dialect::{Dialect, DialectProvider},
    Config, Result, Session, TenxError,
};

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const MAX_TOKENS: u32 = 8192;
const CONTEXT_LEADIN: &str = "Here is some immutable context that you may not edit.\n";

use tokio::sync::mpsc;

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

    fn extract_changes(&self) -> Result<changes::ChangeSet> {
        let mut cset = changes::ChangeSet::default();
        for message in &self.conversation.messages {
            if message.role == Role::Assistant {
                for content in &message.content {
                    if let Content::Text { text } = content {
                        let parsed_ops = parse_response_text(text)?;
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
        let context = session.dialect.render_context(session)?;
        if session.context.is_empty() {
            // Remove existing context messages if present
            self.conversation.messages.retain(|msg|
                !(msg.role == misanthropy::Role::User && msg.content.iter().any(|c|
                    matches!(c, misanthropy::Content::Text { text } if text.starts_with(CONTEXT_LEADIN))
                ))
            );
            return Ok(());
        }

        let ctx_u = misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text {
                text: format!("{}\n{}", CONTEXT_LEADIN, context),
            }],
        };
        let ctx_a = misanthropy::Message {
            role: misanthropy::Role::Assistant,
            content: vec![misanthropy::Content::Text {
                text: "Got it. What would you like me to do?".to_string(),
            }],
        };

        if self.conversation.messages.is_empty() {
            // Append context messages if conversation is empty
            self.conversation.messages = vec![ctx_u, ctx_a];
        } else if self.conversation.messages.len() >= 2
            && self.conversation.messages[0].role == misanthropy::Role::User
            && self.conversation.messages[0].content.iter().any(|c|
                matches!(c, misanthropy::Content::Text { text } if text.starts_with(CONTEXT_LEADIN))
            ) {
            // Replace existing context messages
            self.conversation.messages[0] = ctx_u;
            self.conversation.messages[1] = ctx_a;
        } else {
            // Insert context messages at the start
            self.conversation.messages.insert(0, ctx_a);
            self.conversation.messages.insert(0, ctx_u);
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
    ) -> Result<changes::ChangeSet> {
        self.conversation.system = Some(dialect.system());
        let prompt = session
            .prompt_inputs
            .last()
            .ok_or(TenxError::Internal("no prompt inputs".into()))?;

        self.update_context_messages(session)?;

        let txt = dialect.render_prompt(prompt)?;
        self.conversation.messages.push(misanthropy::Message {
            role: misanthropy::Role::User,
            content: vec![misanthropy::Content::Text { text: txt }],
        });

        let resp = self.stream_response(&config.anthropic_key, sender).await?;
        self.conversation.merge_response(&resp);
        self.extract_changes()
    }
}

/// Parses a response string containing XML-like tags and returns a `ChangeSet` struct.
///
/// The input string should contain one or more of the following tags:
///
/// `<write_file>` tag for file content:
/// ```xml
/// <write_file path="/path/to/file.txt">
///     File content goes here
/// </write_file>
/// ```
///
/// `<replace>` tag for file replace:
/// ```xml
/// <replace path="/path/to/file.txt">
///     <old>Old content goes here</old>
///     <new>New content goes here</new>
/// </replace>
/// ```
///
/// The function parses these tags and populates a `ChangeSet` struct with
/// `WriteFile` entries for `<write_file>` tags and `Replace` entries for `<replace>` tags.
/// Whitespace is trimmed from the content of all tags. Any text outside of recognized tags is
/// ignored.
pub fn parse_response_text(response: &str) -> Result<changes::ChangeSet> {
    let mut cset = changes::ChangeSet::default();
    let mut lines = response.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("<write_file ") {
            let path = extract_path(trimmed)?;
            let content = parse_content(&mut lines, "write_file")?;
            cset.changes
                .push(changes::Change::Write(changes::WriteFile {
                    path: path.into(),
                    content,
                }));
        } else if trimmed.starts_with("<replace ") {
            let path = extract_path(trimmed)?;
            let old = parse_nested_content(&mut lines, "old")?;
            let new = parse_nested_content(&mut lines, "new")?;
            cset.changes
                .push(changes::Change::Replace(changes::Replace {
                    path: path.into(),
                    old,
                    new,
                }));
        }
        // Ignore other lines
    }

    Ok(cset)
}

fn extract_path(line: &str) -> Result<String> {
    let start = line
        .find("path=\"")
        .ok_or_else(|| TenxError::Parse("Missing path attribute".to_string()))?;
    let end = line[start + 6..]
        .find('"')
        .ok_or_else(|| TenxError::Parse("Malformed path attribute".to_string()))?;
    Ok(line[start + 6..start + 6 + end].to_string())
}

fn parse_content<'a, I>(lines: &mut I, end_tag: &str) -> Result<String>
where
    I: Iterator<Item = &'a str>,
{
    let mut content = String::new();
    for line in lines {
        if line.trim() == format!("</{}>", end_tag) {
            return Ok(content.trim().to_string());
        }
        content.push_str(line);
        content.push('\n');
    }
    Err(TenxError::Parse(format!(
        "Missing closing tag for {}",
        end_tag
    )))
}

fn parse_nested_content<'a, I>(lines: &mut I, tag: &str) -> Result<String>
where
    I: Iterator<Item = &'a str>,
{
    let opening_tag = format!("<{}>", tag);
    let closing_tag = format!("</{}>", tag);

    // Skip lines until we find the opening tag
    for line in lines.by_ref() {
        if line.trim() == opening_tag {
            break;
        }
    }

    let mut content = String::new();
    for line in lines {
        if line.trim() == closing_tag {
            return Ok(content.trim().to_string());
        }
        content.push_str(line);
        content.push('\n');
    }
    Err(TenxError::Parse(format!("Missing closing tag for {}", tag)))
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
        assert!(claude.conversation.messages.is_empty());

        // Test adding context to empty conversation
        session.add_context(Context {
            ty: crate::session::ContextType::File,
            name: "test".to_string(),
            data: crate::session::ContextData::String("Test context".to_string()),
        });
        claude.update_context_messages(&session)?;
        assert_eq!(claude.conversation.messages.len(), 2);
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
        assert_eq!(claude.conversation.messages.len(), 2);
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

        assert_eq!(claude.conversation.messages.len(), 3);
        if let misanthropy::Content::Text { text } = &claude.conversation.messages[0].content[0] {
            assert!(text.starts_with(CONTEXT_LEADIN));
        } else {
            panic!("Expected Text content");
        }
        if let misanthropy::Content::Text { text } = &claude.conversation.messages[2].content[0] {
            assert_eq!(text, "User message");
        } else {
            panic!("Expected Text content");
        }

        Ok(())
    }

    #[test]
    fn test_parse_response_basic() {
        let input = r#"
            ignored
            <write_file path="/path/to/file2.txt">
                This is the content of the file.
            </write_file>
            ignored
            <replace path="/path/to/file.txt">
                <old>
                Old content
                </old>
                <new>
                New content
                </new>
            </replace>
            ignored
        "#;

        let result = parse_response_text(input).unwrap();
        assert_eq!(result.changes.len(), 2);

        match &result.changes[0] {
            changes::Change::Write(write_file) => {
                assert_eq!(write_file.path.as_os_str(), "/path/to/file2.txt");
                assert_eq!(
                    write_file.content.trim(),
                    "This is the content of the file."
                );
            }
            _ => panic!("Expected WriteFile for /path/to/file2.txt"),
        }

        match &result.changes[1] {
            changes::Change::Replace(replace) => {
                assert_eq!(replace.path.as_os_str(), "/path/to/file.txt");
                assert_eq!(replace.old.trim(), "Old content");
                assert_eq!(replace.new.trim(), "New content");
            }
            _ => panic!("Expected Replace for /path/to/file.txt"),
        }
    }
}
