use crate::{Result, TenxError};
use misanthropy::{Content, MessagesRequest, Role};
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::Reader;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WriteFile {
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub old: String,
    pub new: String,
}

impl Diff {
    pub fn apply(&self, input: &str) -> Result<String> {
        let old_lines: Vec<&str> = self.old.lines().map(str::trim).collect();
        let new_lines: Vec<&str> = self.new.lines().collect();
        let input_lines: Vec<&str> = input.lines().collect();

        let mut result = Vec::new();
        let mut i = 0;

        while i < input_lines.len() {
            if input_lines[i..]
                .iter()
                .map(|s| s.trim())
                .collect::<Vec<_>>()
                .starts_with(&old_lines)
            {
                // Found a match, replace with new content
                result.extend(new_lines.iter().cloned());
                i += old_lines.len();
            } else {
                // No match, keep the original line
                result.push(input_lines[i]);
                i += 1;
            }
        }

        if result == input_lines {
            Err(TenxError::Operation("No changes were applied".to_string()))
        } else {
            Ok(result.join("\n"))
        }
    }
}

#[derive(Debug, Clone)]
pub enum Operation {
    Write(WriteFile),
    Diff(Diff),
}

#[derive(Debug)]
pub struct Operations {
    pub operations: HashMap<String, Operation>,
}

impl Operations {
    fn new() -> Self {
        Operations {
            operations: HashMap::new(),
        }
    }
}

pub fn extract_operations(request: &MessagesRequest) -> Result<Operations> {
    let mut operations = Operations::new();
    for message in &request.messages {
        if message.role == Role::Assistant {
            for content in &message.content {
                if let Content::Text { text } = content {
                    let parsed_ops = parse_response_text(text)?;
                    operations.operations.extend(parsed_ops.operations);
                }
            }
        }
    }

    Ok(operations)
}

/// Parses a response string containing XML-like tags and returns a `Response` struct.
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
/// `<diff>` tag for file differences:
/// ```xml
/// <diff path="/path/to/diff_file.txt">
///     <old>Old content goes here</old>
///     <new>New content goes here</new>
/// </diff>
/// ```
///
/// The function parses these tags and populates a `Response` struct with
/// `WriteFile` entries for `<write_file>` tags and `Diff` entries for `<diff>` tags.
/// Whitespace is trimmed from the content of all tags. Any text outside of recognized tags is
/// ignored.
pub fn parse_response_text(response: &str) -> Result<Operations> {
    let mut reader = Reader::from_str(response);
    reader.config_mut().trim_text(true);

    let mut operations = Operations::new();

    let mut buf = Vec::new();
    let mut current_tag = String::new();
    let mut current_path = String::new();
    let mut current_old = String::new();
    let mut current_new = String::new();
    let mut in_old = false;
    let mut in_new = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"write_file" | b"diff" => {
                        current_tag = std::str::from_utf8(name.as_ref())
                            .map_err(|e| TenxError::Parse(e.to_string()))?
                            .to_string();
                        current_path = get_path_attribute(e)?;
                    }
                    b"old" => in_old = true,
                    b"new" => in_new = true,
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let content = e.unescape().map_err(|e| TenxError::Parse(e.to_string()))?;
                match current_tag.as_str() {
                    "write_file" => {
                        let write_file = WriteFile {
                            content: content.trim().to_string(),
                        };
                        operations
                            .operations
                            .insert(current_path.clone(), Operation::Write(write_file));
                    }
                    "diff" => {
                        if in_old {
                            current_old = content.trim().to_string();
                        } else if in_new {
                            current_new = content.trim().to_string();
                        }
                    }
                    _ => {} // Discard text outside of recognized tags
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"diff" => {
                        let diff = Diff {
                            old: current_old.clone(),
                            new: current_new.clone(),
                        };
                        operations
                            .operations
                            .insert(current_path.clone(), Operation::Diff(diff));
                        current_old.clear();
                        current_new.clear();
                    }
                    b"old" => in_old = false,
                    b"new" => in_new = false,
                    _ => {}
                }
                if name.as_ref() == b"write_file" || name.as_ref() == b"diff" {
                    current_tag.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(TenxError::Parse(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(operations)
}

fn get_path_attribute(e: &BytesStart) -> Result<String> {
    let path_attr = e
        .attributes()
        .find(|a| a.as_ref().map(|a| a.key == QName(b"path")).unwrap_or(false))
        .ok_or_else(|| TenxError::Parse("Missing path attribute".to_string()))?;

    let path_value = path_attr
        .map_err(|e| TenxError::Parse(e.to_string()))?
        .unescape_value()
        .map_err(|e| TenxError::Parse(e.to_string()))?;

    Ok(path_value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_basic() {
        let input = r#"
            ignored
            <write_file path="/path/to/file.txt">
                This is the content of the file.
            </write_file>
            ignored
            <diff path="/path/to/diff_file.txt">
                <old>Old content</old>
                <new>New content</new>
            </diff>
            ignored
        "#;

        let result = parse_response_text(input).unwrap();

        assert_eq!(result.operations.len(), 2);

        match result.operations.get("/path/to/file.txt") {
            Some(Operation::Write(write_file)) => {
                assert_eq!(
                    write_file.content.trim(),
                    "This is the content of the file."
                );
            }
            _ => panic!("Expected WriteFile operation for /path/to/file.txt"),
        }

        match result.operations.get("/path/to/diff_file.txt") {
            Some(Operation::Diff(diff)) => {
                assert_eq!(diff.old.trim(), "Old content");
                assert_eq!(diff.new.trim(), "New content");
            }
            _ => panic!("Expected Diff operation for /path/to/diff_file.txt"),
        }
    }

    #[test]
    fn test_diff_apply() {
        let diff = Diff {
            old: "This is\nold content\nto be replaced".to_string(),
            new: "This is\nnew content\nthat replaces the old".to_string(),
        };

        let input = "Some initial text\nThis is\nold content\nto be replaced\nSome final text";
        let expected_output =
            "Some initial text\nThis is\nnew content\nthat replaces the old\nSome final text";

        let result = diff.apply(input).expect("Failed to apply diff");
        assert_eq!(result, expected_output);
    }

    #[test]
    fn test_diff_apply_whitespace_insensitive() {
        let diff = Diff {
            old: "  This is\n  old content  \nto be replaced  ".to_string(),
            new: "This is\nnew content\nthat replaces the old".to_string(),
        };

        let input =
            "Some initial text\n This is \n   old content\n  to be replaced \nSome final text";
        let expected_output =
            "Some initial text\nThis is\nnew content\nthat replaces the old\nSome final text";

        let result = diff.apply(input).expect("Failed to apply diff");
        assert_eq!(result, expected_output);
    }
}
