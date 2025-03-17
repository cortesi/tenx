use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// An exact replace operation that replaces one occurrence of a string with another.
/// The match must be exact and appear exactly once in the file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Replace {
    pub path: PathBuf,
    pub old: String,
    pub new: String,
}

impl Replace {
    /// Applies the replacement operation to the given input string.
    ///
    /// Replaces exactly one occurrence of the old content with the new content.
    /// Returns an error if the old content is not found exactly once.
    pub fn apply(&self, input: &str) -> Result<String> {
        match input.matches(&self.old).count() {
            0 => Err(Error::Patch {
                user: "Text to replace not found".to_string(),
                model: format!(
                    "Could not find the specified text in the source file:\n{}",
                    self.old
                ),
            }),
            1 => Ok(input.replace(&self.old, &self.new)),
            _ => Err(Error::Patch {
                user: "Multiple occurrences of text to replace found".to_string(),
                model: format!(
                    "Found multiple occurrences of the specified text in the source file:\n{}",
                    self.old
                ),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_replace_apply() {
        // Successful replace
        let replace = Replace {
            path: PathBuf::from("/path/to/file.txt"),
            old: "old content".to_string(),
            new: "new content".to_string(),
        };

        let input = "before old content after";
        let result = replace.apply(input).unwrap();
        assert_eq!(result, "before new content after");

        // Not found
        let replace = Replace {
            path: PathBuf::from("/path/to/file.txt"),
            old: "nonexistent".to_string(),
            new: "new".to_string(),
        };
        assert!(replace.apply(input).is_err());

        // Multiple occurrences
        let replace = Replace {
            path: PathBuf::from("/path/to/file.txt"),
            old: "o".to_string(),
            new: "x".to_string(),
        };
        assert!(replace.apply(input).is_err());
    }
}
