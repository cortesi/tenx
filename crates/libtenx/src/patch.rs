use crate::{Result, TenxError};
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

        Err(TenxError::Patch {
            user: "Could not find the text to replace".to_string(),
            model: format!(
                "Invalid replace specification - could not find the following text in the source file:\n{}",
                self.old
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub path: PathBuf,
    pub block: String,
}

impl Block {
    pub fn apply(&self, input: &str) -> Result<String> {
        let block_lines: Vec<&str> = self.block.lines().collect();
        let input_lines: Vec<&str> = input.lines().collect();

        if block_lines.is_empty() {
            return Ok(input.to_string());
        }

        let start_line = block_lines[0].trim();
        let mut start_index = None;

        for (i, line) in input_lines.iter().enumerate() {
            if line.trim() == start_line {
                if start_index.is_some() {
                    return Err(TenxError::Patch {
                        user: "Multiple matches found for the block start".to_string(),
                        model: "The first line of the block appears multiple times in the input"
                            .to_string(),
                    });
                }
                start_index = Some(i);
            }
        }

        let start_index = start_index.ok_or_else(|| TenxError::Patch {
            user: "Could not find the block to replace".to_string(),
            model: "The first line of the block does not appear in the input".to_string(),
        })?;

        let mut end_index = start_index;
        let start_indent =
            input_lines[start_index].len() - input_lines[start_index].trim_start().len();

        for (i, line) in input_lines.iter().enumerate().skip(start_index + 1) {
            let line_indent = line.len() - line.trim_start().len();
            if line_indent <= start_indent && (line.trim() == "}" || line.trim().ends_with(":")) {
                end_index = i;
                break;
            }
            end_index = i;
        }

        let mut result = input_lines[..start_index].join("\n");
        if !result.is_empty() {
            result.push('\n');
        }
        let indented_block = block_lines
            .iter()
            .map(|line| format!("{}{}", " ".repeat(start_indent), line))
            .collect::<Vec<_>>()
            .join("\n");
        result.push_str(&indented_block);
        if end_index < input_lines.len() - 1 {
            result.push('\n');
            result.push_str(&input_lines[end_index + 1..].join("\n"));
        }

        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Change {
    Write(WriteFile),
    Replace(Replace),
    Block(Block),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Patch {
    pub changes: Vec<Change>,
    pub comment: Option<String>,
    pub cache: HashMap<PathBuf, String>,
}

impl Patch {
    /// Returns a vector of PathBufs for all files changed in the patch.
    pub fn changed_files(&self) -> Vec<PathBuf> {
        self.changes
            .iter()
            .map(|change| match change {
                Change::Write(write_file) => write_file.path.clone(),
                Change::Replace(replace) => replace.path.clone(),
                Change::Block(block) => block.path.clone(),
            })
            .collect()
    }

    /// Returns a string representation of the change for display purposes.
    pub fn change_description(change: &Change) -> String {
        match change {
            Change::Write(write_file) => format!("Write to {}", write_file.path.display()),
            Change::Replace(replace) => format!("Replace in {}", replace.path.display()),
            Change::Block(block) => format!("Block in {}", block.path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_changed_files() {
        let mut patch = Patch::default();
        patch.changes.push(Change::Write(WriteFile {
            path: PathBuf::from("file1.txt"),
            content: "content".to_string(),
        }));
        patch.changes.push(Change::Replace(Replace {
            path: PathBuf::from("file2.txt"),
            old: "old".to_string(),
            new: "new".to_string(),
        }));

        let changed_files = patch.changed_files();
        assert_eq!(changed_files.len(), 2);
        assert!(changed_files.contains(&PathBuf::from("file1.txt")));
        assert!(changed_files.contains(&PathBuf::from("file2.txt")));
    }

    #[test]
    fn test_replace_apply() {
        let replace = Replace {
            path: "/path/to/file.txt".into(),
            old: indoc! {"
                This is
                old content
                to be replaced
            "}
            .trim()
            .to_string(),
            new: indoc! {"
                This is
                new content
                that replaces the old
            "}
            .trim()
            .to_string(),
        };

        let input = indoc! {"
            Some initial text
            This is
            old content
            to be replaced
            Some final text
            This is
            old content
            to be replaced
            More text
        "};
        let expected_output = indoc! {"
            Some initial text
            This is
            new content
            that replaces the old
            Some final text
            This is
            old content
            to be replaced
            More text
        "};

        let result = replace.apply(input).expect("Failed to apply replace");
        assert_eq!(result, expected_output.trim_end());
    }

    #[test]
    fn test_replace_apply_whitespace_insensitive() {
        let replace = Replace {
            path: "/path/to/file.txt".into(),
            old: indoc! {"
                  This is
                  old content  
                to be replaced
            "}
            .trim()
            .to_string(),
            new: indoc! {"
                This is
                new content
                that replaces the old
            "}
            .trim()
            .to_string(),
        };

        let input = indoc! {"
            Some initial text
             This is 
               old content
              to be replaced 
            Some final text
        "};
        let expected_output = indoc! {"
            Some initial text
            This is
            new content
            that replaces the old
            Some final text
        "};

        let result = replace.apply(input).expect("Failed to apply replace");
        assert_eq!(result, expected_output.trim_end());
    }

    #[test]
    fn test_block_apply() {
        let block = Block {
            path: "/path/to/file.txt".into(),
            block: indoc! {"
                    fn foo() {
                        println!('something else!');
                    }
            "}
            .trim()
            .to_string(),
        };

        let input = indoc! {"
            fn foo() {
                println!('hello');
            }
            fn bar () {
                println!('hi there');
            }
        "};
        let expected_output = indoc! {"
            fn foo() {
                println!('something else!');
            }
            fn bar () {
                println!('hi there');
            }
        "};

        let result = block.apply(input).expect("Failed to apply block");
        assert_eq!(result, expected_output.trim_end());
    }

    #[test]
    fn test_block_apply_corner_cases() {
        // Case 1: Block at the beginning of the file
        let block1 = Block {
            path: "/path/to/file.txt".into(),
            block: indoc! {"
                fn first_function() {
                    // New implementation
                }
            "}
            .trim()
            .to_string(),
        };

        let input1 = indoc! {"
            fn first_function() {
                // Old implementation
            }
            
            fn second_function() {
                // Some code
            }
        "};
        let expected_output1 = indoc! {"
            fn first_function() {
                // New implementation
            }
            
            fn second_function() {
                // Some code
            }
        "};

        let result1 = block1
            .apply(input1)
            .expect("Failed to apply block at the beginning");
        assert_eq!(result1, expected_output1.trim_end());

        // Case 2: Block at the end of the file
        let block2 = Block {
            path: "/path/to/file.txt".into(),
            block: indoc! {"
                fn last_function() {
                    println!('New last function');
                }
            "}
            .trim()
            .to_string(),
        };

        let input2 = indoc! {"
            fn first_function() {
                // Some code
            }
            
            fn last_function() {
                // Old implementation
            }
        "};
        let expected_output2 = indoc! {"
            fn first_function() {
                // Some code
            }
            
            fn last_function() {
                println!('New last function');
            }
        "};

        let result2 = block2
            .apply(input2)
            .expect("Failed to apply block at the end");
        assert_eq!(result2, expected_output2.trim_end());

        // Case 3: Block with different indentation
        let block3 = Block {
            path: "/path/to/file.txt".into(),
            block: indoc! {"
                    fn indented_function() {
                        println!('New indented function');
                    }
            "}
            .trim()
            .to_string(),
        };

        let input3 = indoc! {"
            fn first_function() {
                // Some code
            }
            
                fn indented_function() {
                    // Old implementation
                }
            
            fn last_function() {
                // Some code
            }
        "};
        let expected_output3 = indoc! {"
            fn first_function() {
                // Some code
            }
            
                fn indented_function() {
                    println!('New indented function');
                }
            
            fn last_function() {
                // Some code
            }
        "};

        let result3 = block3
            .apply(input3)
            .expect("Failed to apply block with different indentation");
        assert_eq!(result3, expected_output3.trim_end());
    }
}
