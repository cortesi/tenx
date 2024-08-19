use crate::{Result, TenxError};
use std::collections::HashMap;
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

        Err(TenxError::Retry {
            user: "Could not find the text to replace".to_string(),
            model: "Invalid replace specification - could not find the text to replace".to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Change {
    Write(WriteFile),
    Replace(Replace),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub changes: Vec<Change>,
    pub comment: Option<String>,
    pub cache: HashMap<PathBuf, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

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
