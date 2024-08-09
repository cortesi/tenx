//! Defines an interaction style where files are sent to the model in XML-like tags,
//! and model responses are parsed from similar tags.

use crate::{Context, Operation, Operations, Replace, Result, TenxError, WriteFile};

use super::{Dialect, Prompt};

const SYSTEM: &str = include_str!("./tags-system.txt");

pub struct Tags {}

impl Dialect for Tags {
    fn system(&self) -> String {
        SYSTEM.to_string()
    }

    fn render(&self, ctx: Context, p: &Prompt) -> Result<String> {
        let mut rendered = String::new();

        // Add editable files
        for path in &p.edit_paths {
            let contents = ctx.workspace.read_file(path)?;
            rendered.push_str(&format!(
                "\n<editable path=\"{}\">\n{}</editable>\n\n",
                path.display(),
                contents
            ));
        }

        // Add context files
        for path in &p.attach_paths {
            let contents = ctx.workspace.read_file(path)?;
            rendered.push_str(&format!(
                "\n<context path=\"{}\">\n{}</context>\n\n",
                path.display(),
                contents
            ));
        }

        // Add user prompt
        rendered.push_str(&p.user_prompt);
        Ok(rendered)
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
    fn parse(&self, response: &str) -> Result<Operations> {
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
}