use crate::{Result, TenxError};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replace {
    pub path: PathBuf,
    pub old: String,
    pub new: String,
}

impl Replace {
    /// Applies the replacement operation to the given input string.
    ///
    /// Replaces only the first occurrence of the old content with the new content.
    /// Returns the modified string if the replacement was successful, or an error if no changes were made.
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
                result.extend(new_lines.iter().cloned());
                result.extend(input_lines[i + old_lines.len()..].iter().cloned());
                return Ok(result.join("\n"));
            } else {
                result.push(input_lines[i]);
                i += 1;
            }
        }

        Err(TenxError::Operation(
            "Could not find the text to replace".to_string(),
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Write(WriteFile),
    Replace(Replace),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Operations {
    pub operations: Vec<Operation>,
}

/// Parses a response string containing XML-like tags and returns a `Operations` struct.
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
/// The function parses these tags and populates an `Operations` struct with
/// `WriteFile` entries for `<write_file>` tags and `Replace` entries for `<replace>` tags.
/// Whitespace is trimmed from the content of all tags. Any text outside of recognized tags is
/// ignored.
pub fn parse_response_text(response: &str) -> Result<Operations> {
    let mut operations = Operations::default();
    let mut lines = response.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("<write_file ") {
            let path = extract_path(trimmed)?;
            let content = parse_content(&mut lines, "write_file")?;
            operations.operations.push(Operation::Write(WriteFile {
                path: path.into(),
                content,
            }));
        } else if trimmed.starts_with("<replace ") {
            let path = extract_path(trimmed)?;
            let old = parse_nested_content(&mut lines, "old")?;
            let new = parse_nested_content(&mut lines, "new")?;
            operations.operations.push(Operation::Replace(Replace {
                path: path.into(),
                old,
                new,
            }));
        }
        // Ignore other lines
    }

    Ok(operations)
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
    use pretty_assertions::assert_eq;

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
        assert_eq!(result.operations.len(), 2);

        match &result.operations[0] {
            Operation::Write(write_file) => {
                assert_eq!(write_file.path.as_os_str(), "/path/to/file2.txt");
                assert_eq!(
                    write_file.content.trim(),
                    "This is the content of the file."
                );
            }
            _ => panic!("Expected WriteFile operation for /path/to/file2.txt"),
        }

        match &result.operations[1] {
            Operation::Replace(replace) => {
                assert_eq!(replace.path.as_os_str(), "/path/to/file.txt");
                assert_eq!(replace.old.trim(), "Old content");
                assert_eq!(replace.new.trim(), "New content");
            }
            _ => panic!("Expected Replace operation for /path/to/file.txt"),
        }
    }

    #[test]
    fn test_replace_apply() {
        let replace = Replace {
            path: "/path/to/file.txt".into(),
            old: "This is\nold content\nto be replaced".to_string(),
            new: "This is\nnew content\nthat replaces the old".to_string(),
        };

        let input = "Some initial text\nThis is\nold content\nto be replaced\nSome final text\nThis is\nold content\nto be replaced\nMore text";
        let expected_output = "Some initial text\nThis is\nnew content\nthat replaces the old\nSome final text\nThis is\nold content\nto be replaced\nMore text";

        let result = replace.apply(input).expect("Failed to apply replace");
        assert_eq!(result, expected_output);
    }

    #[test]
    fn test_replace_apply_whitespace_insensitive() {
        let replace = Replace {
            path: "/path/to/file.txt".into(),
            old: "  This is\n  old content  \nto be replaced  ".to_string(),
            new: "This is\nnew content\nthat replaces the old".to_string(),
        };

        let input =
            "Some initial text\n This is \n   old content\n  to be replaced \nSome final text";
        let expected_output =
            "Some initial text\nThis is\nnew content\nthat replaces the old\nSome final text";

        let result = replace.apply(input).expect("Failed to apply replace");
        assert_eq!(result, expected_output);
    }
}
