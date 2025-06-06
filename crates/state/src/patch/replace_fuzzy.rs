use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::PatchError;

/// An replace operation that replaces once occurrence of a string with another. This operation is
/// fuzzy - meaning it tries really hard to make the replacement by ignoring leading and trailing
/// whitespace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplaceFuzzy {
    pub path: PathBuf,
    pub old: String,
    pub new: String,
}

impl ReplaceFuzzy {
    /// Applies the replacement operation to the given input string.
    ///
    /// Replaces only the first occurrence of the old content with the new content.
    /// Returns the modified string if the replacement was successful, or an error if no changes were made.
    pub(crate) fn apply(&self, input: &str) -> Result<String, PatchError> {
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

        Err(PatchError {
            user: "Could not find the text to replace".to_string(),
            model: format!(
                "Invalid replace specification - could not find the following text in the source file:\n{}",
                self.old
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_replace_apply() {
        let test_cases = vec![
            (
                "Basic replace",
                "/path/to/file.txt",
                indoc! {"
                    This is
                    old content
                    to be replaced
                "},
                indoc! {"
                    This is
                    new content
                    that replaces the old
                "},
                indoc! {"
                    Some initial text
                    This is
                    old content
                    to be replaced
                    Some final text
                    This is
                    old content
                    to be replaced
                    More text
                "},
                indoc! {"
                    Some initial text
                    This is
                    new content
                    that replaces the old
                    Some final text
                    This is
                    old content
                    to be replaced
                    More text
                "},
            ),
            (
                "Whitespace insensitive replace",
                "/path/to/file.txt",
                indoc! {"
                      This is
                      old content  
                    to be replaced
                "},
                indoc! {"
                    This is
                    new content
                    that replaces the old
                "},
                indoc! {"
                    Some initial text
                     This is 
                       old content
                      to be replaced 
                    Some final text
                "},
                indoc! {"
                    Some initial text
                    This is
                    new content
                    that replaces the old
                    Some final text
                "},
            ),
        ];

        for (name, path, old, new, input, expected_output) in test_cases {
            let replace = ReplaceFuzzy {
                path: path.into(),
                old: old.trim().to_string(),
                new: new.trim().to_string(),
            };

            let result = replace
                .apply(input)
                .unwrap_or_else(|_| panic!("Failed to apply replace: {name}"));
            assert_eq!(result, expected_output.trim_end(), "Test case: {}", name);
        }
    }
}
