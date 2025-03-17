use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

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
    pub fn apply(&self, input: &str) -> Result<String> {
        let lines: Vec<&str> = input.lines().collect();

        if self.line > lines.len() {
            return Err(Error::Patch {
                user: format!("Line {} is out of bounds", self.line),
                model: format!(
                    "Cannot insert at line {} because the file only has {} lines",
                    self.line,
                    lines.len()
                ),
            });
        }

        let mut result = Vec::new();

        // Add lines before insertion point
        result.extend(lines[..self.line].iter().cloned());

        // Add the new content
        result.extend(self.new.lines());

        // Add lines after insertion point
        result.extend(lines[self.line..].iter().cloned());

        Ok(result.join("\n"))
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
            new: "inserted content".to_string(),
        };

        let input = "line 1\nline 2\nline 3";
        let result = insert.apply(input).unwrap();
        assert_eq!(result, "inserted content\nline 1\nline 2\nline 3");

        // Insert in the middle
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 1,
            new: "inserted content".to_string(),
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
            new: "inserted line 1\ninserted line 2".to_string(),
        };

        let result = insert.apply(input).unwrap();
        assert_eq!(
            result,
            "line 1\ninserted line 1\ninserted line 2\nline 2\nline 3"
        );

        // Line out of bounds
        let insert = Insert {
            path: PathBuf::from("/path/to/file.txt"),
            line: 4,
            new: "inserted content".to_string(),
        };

        assert!(insert.apply(input).is_err());
    }
}
