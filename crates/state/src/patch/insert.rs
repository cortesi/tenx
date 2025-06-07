use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::PatchError;

/// An insert operation that adds text at a specific line in a file.
/// Offset 0 is the start of the file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Insert {
    pub path: PathBuf,
    pub line: usize,
    pub new: String,
}

impl Insert {
    /// Applies the insert operation to the given input string.
    ///
    /// Inserts the new content at the specified line.
    /// Returns an error if the line number is out of bounds.
    pub(crate) fn apply(&self, input: &str) -> Result<String, PatchError> {
        let lines: Vec<&str> = input.lines().collect();

        if self.line > lines.len() {
            return Err(PatchError {
                user: format!("Line {} is out of bounds", self.line),
                model: format!(
                    "Cannot insert at line {} because the file only has {} lines",
                    self.line,
                    lines.len()
                ),
            });
        }

        // Find the offset at the target line
        let mut offset = 0;
        for line in lines.iter().take(self.line) {
            offset += line.len() + 1; // +1 for the newline
        }

        // Handle the case where we're inserting at the end of the file
        if self.line == lines.len() && !input.is_empty() && !input.ends_with('\n') {
            // If the file doesn't end with a newline, add one before inserting
            let mut result = input.to_string();
            result.push('\n');
            result.insert_str(result.len(), &self.new);
            return Ok(result);
        }

        // Insert the new content at the offset
        let mut result = input.to_string();
        result.insert_str(offset, &self.new);

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_insert_apply() {
        // Insert at beginning
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 0,
            new: "inserted content\n".to_string(),
        };

        let input = "line 1\nline 2\nline 3";
        let result = insert.apply(input).unwrap();
        assert_eq!(result, "inserted content\nline 1\nline 2\nline 3");

        // Insert in the middle
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 1,
            new: "inserted content\n".to_string(),
        };

        let result = insert.apply(input).unwrap();
        assert_eq!(result, "line 1\ninserted content\nline 2\nline 3");

        // Insert at the end
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 3,
            new: "inserted content".to_string(),
        };

        let result = insert.apply(input).unwrap();
        assert_eq!(result, "line 1\nline 2\nline 3\ninserted content");

        // Multi-line insert
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 1,
            new: "inserted line 1\ninserted line 2\n".to_string(),
        };

        let result = insert.apply(input).unwrap();
        assert_eq!(
            result,
            "line 1\ninserted line 1\ninserted line 2\nline 2\nline 3"
        );

        // Insert with trailing newlines preserved
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 1,
            new: "inserted content\n\n".to_string(),
        };

        let result = insert.apply(input).unwrap();
        assert_eq!(result, "line 1\ninserted content\n\nline 2\nline 3");

        // Line out of bounds
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 4,
            new: "inserted content".to_string(),
        };

        assert!(insert.apply(input).is_err());
    }
}
