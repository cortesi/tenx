//! Defines an interaction style where files are sent to the model in XML-like tags, and model
//! responses are parsed from similar tags.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::{xmlish, DialectProvider, PromptInput};
use crate::{Change, Patch, Replace, Result, Session, TenxError, WriteFile};

const SYSTEM: &str = include_str!("./tags-system.txt");

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tags {}

impl DialectProvider for Tags {
    fn system(&self) -> String {
        SYSTEM.to_string()
    }

    fn render_context(&self, s: &Session) -> Result<String> {
        if self.system().is_empty() {
            return Ok("There is no non-editable context.".into());
        }

        let mut rendered = String::new();
        rendered.push_str("<context>\n");
        for ctx in &s.context {
            rendered.push_str(&format!(
                "<item name=\"{}\" type=\"{:?}\">\n{}\n</item>\n",
                ctx.name,
                ctx.ty,
                ctx.body()?
            ));
        }
        rendered.push_str("</context>");
        Ok(rendered)
    }

    fn render_editables(&self, paths: Vec<PathBuf>) -> Result<String> {
        let mut rendered = String::new();
        for path in paths {
            let contents = fs::read_to_string(&path)?;
            rendered.push_str(&format!(
                "<editable path=\"{}\">\n{}</editable>\n\n",
                path.display(),
                contents
            ));
        }
        Ok(rendered)
    }

    fn render_prompt(&self, p: &PromptInput) -> Result<String> {
        let mut rendered = String::new();
        rendered.push_str(&format!("\n<prompt>\n{}\n</prompt>\n\n", p.user_prompt));
        Ok(rendered)
    }

    /// Parses a response string containing XML-like tags and returns a `Patch` struct.
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
    /// The function parses these tags and populates an `Patch` struct with
    /// `WriteFile` entries for `<write_file>` tags and `Replace` entries for `<replace>` tags.
    /// Whitespace is trimmed from the content of all tags. Any text outside of recognized tags is
    /// ignored.
    fn parse(&self, response: &str) -> Result<Patch> {
        let mut change_set = Patch::default();
        let mut lines = response.lines().map(String::from).peekable();

        while let Some(line) = lines.peek() {
            if let Some(tag) = xmlish::parse_open(line) {
                match tag.name.as_str() {
                    "write_file" => {
                        let path = tag
                            .attributes
                            .get("path")
                            .ok_or_else(|| TenxError::Parse("Missing path attribute".into()))?
                            .clone();
                        let (_, content) = xmlish::parse_block("write_file", &mut lines)?;
                        change_set.changes.push(Change::Write(WriteFile {
                            path: path.into(),
                            content: content.join("\n"),
                        }));
                    }
                    "replace" => {
                        let path = tag
                            .attributes
                            .get("path")
                            .ok_or_else(|| TenxError::Parse("Missing path attribute".into()))?
                            .clone();
                        let (_, replace_content) = xmlish::parse_block("replace", &mut lines)?;
                        let mut replace_lines = replace_content.into_iter().peekable();
                        let (_, old) = xmlish::parse_block("old", &mut replace_lines)?;
                        let (_, new) = xmlish::parse_block("new", &mut replace_lines)?;
                        change_set.changes.push(Change::Replace(Replace {
                            path: path.into(),
                            old: old.join("\n"),
                            new: new.join("\n"),
                        }));
                    }
                    _ => {
                        lines.next();
                    }
                }
            } else {
                lines.next();
            }
        }
        Ok(change_set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_response_basic() {
        let d = Tags {};

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

        let result = d.parse(input).unwrap();
        assert_eq!(result.changes.len(), 2);

        match &result.changes[0] {
            Change::Write(write_file) => {
                assert_eq!(write_file.path.as_os_str(), "/path/to/file2.txt");
                assert_eq!(
                    write_file.content.trim(),
                    "This is the content of the file."
                );
            }
            _ => panic!("Expected WriteFile for /path/to/file2.txt"),
        }

        match &result.changes[1] {
            Change::Replace(replace) => {
                assert_eq!(replace.path.as_os_str(), "/path/to/file.txt");
                assert_eq!(replace.old.trim(), "Old content");
                assert_eq!(replace.new.trim(), "New content");
            }
            _ => panic!("Expected Replace for /path/to/file.txt"),
        }
    }
}

