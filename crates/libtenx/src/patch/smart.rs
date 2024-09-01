use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, TenxError};

fn smart_ignore_leaders(path: &Path) -> Vec<&'static str> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => vec!["//", "///", "#["],
        Some("go") => vec!["//"],
        Some("py") => vec!["#"],
        Some("c") | Some("h") => vec!["//", "/*"],
        _ => vec!["//", "#", "/*", "#["],
    }
}

/// A smart replacement block. This operation has a set of heuristics for replacing a coherent
/// block of code, without having to specify a file position or the old text (as in Replace). It
/// doess this by detecting the position of the text to be replaced, ignoring common varieties of
/// leading comments, and assuming that the end of the block has equal or lesser indentation as the
/// start of the block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Smart {
    pub path: PathBuf,
    pub text: String,
}

impl Smart {
    pub fn apply(&self, input: &str) -> Result<String> {
        let block_lines: Vec<&str> = self.text.lines().collect();
        let input_lines: Vec<&str> = input.lines().collect();

        if block_lines.is_empty() {
            return Ok(input.to_string());
        }

        let start_line = block_lines[0].trim();
        let mut start_index = None;

        let ignore_leaders = smart_ignore_leaders(&self.path);

        for (i, line) in input_lines.iter().enumerate() {
            if line.trim() == start_line {
                if start_index.is_some() {
                    return Err(TenxError::Patch {
                        user: "Multiple matches found for the smart block start".to_string(),
                        model:
                            "The first line of the smart block appears multiple times in the input"
                                .to_string(),
                    });
                }
                start_index = Some(i);
            }
        }

        if start_index.is_none() {
            let non_ignored_start = block_lines.iter().position(|line| {
                !ignore_leaders
                    .iter()
                    .any(|leader| line.trim().starts_with(leader))
            });
            if let Some(non_ignored_index) = non_ignored_start {
                let non_ignored_line = block_lines[non_ignored_index].trim();
                for (i, line) in input_lines.iter().enumerate() {
                    if line.trim() == non_ignored_line {
                        start_index = Some(i.saturating_sub(non_ignored_index));
                        break;
                    }
                }
            }
        }

        let start_index = start_index.ok_or_else(|| TenxError::Patch {
            user: "Could not find the smart block to replace".to_string(),
            model: "The smart block does not appear in the input".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_smart_apply() {
        let test_cases = vec![
            (
                "Basic smart apply",
                "/path/to/file.txt",
                indoc! {"
                    fn foo() {
                        println!('something else!');
                    }
                "},
                indoc! {"
                    fn foo() {
                        println!('hello');
                    }
                    fn bar () {
                        println!('hi there');
                    }
                "},
                indoc! {"
                    fn foo() {
                        println!('something else!');
                    }
                    fn bar () {
                        println!('hi there');
                    }
                "},
            ),
            (
                "Smart at the beginning of the file",
                "/path/to/file.txt",
                indoc! {"
                    fn first_function() {
                        // New implementation
                    }
                "},
                indoc! {"
                    fn first_function() {
                        // Old implementation
                    }
                    
                    fn second_function() {
                        // Some code
                    }
                "},
                indoc! {"
                    fn first_function() {
                        // New implementation
                    }
                    
                    fn second_function() {
                        // Some code
                    }
                "},
            ),
            (
                "Smart at the end of the file",
                "/path/to/file.txt",
                indoc! {"
                    fn last_function() {
                        println!('New last function');
                    }
                "},
                indoc! {"
                    fn first_function() {
                        // Some code
                    }
                    
                    fn last_function() {
                        // Old implementation
                    }
                "},
                indoc! {"
                    fn first_function() {
                        // Some code
                    }
                    
                    fn last_function() {
                        println!('New last function');
                    }
                "},
            ),
            (
                "Smart with different indentation",
                "/path/to/file.txt",
                indoc! {"
                        fn indented_function() {
                            println!('New indented function');
                        }
                "},
                indoc! {"
                    fn first_function() {
                        // Some code
                    }
                    
                        fn indented_function() {
                            // Old implementation
                        }
                    
                    fn last_function() {
                        // Some code
                    }
                "},
                indoc! {"
                    fn first_function() {
                        // Some code
                    }
                    
                        fn indented_function() {
                            println!('New indented function');
                        }
                    
                    fn last_function() {
                        // Some code
                    }
                "},
            ),
            (
                "Smart with leading comments",
                "/path/to/file.txt",
                indoc! {"
                    /// Updated comment
                    fn foo() {
                        println!(\"hello\")
                    }
                "},
                indoc! {"
                    // Some text
                    /// This is a comment
                    fn foo() {
                    }
                    fn bar() {
                    }
                "},
                indoc! {"
                    // Some text
                    /// Updated comment
                    fn foo() {
                        println!(\"hello\")
                    }
                    fn bar() {
                    }
                "},
            ),
            (
                "Smart with derive macros",
                "/path/to/file.rs",
                indoc! {"
                    #[derive(Debug, Clone)]
                    fn foo() {
                        println!(\"hello from new foo\")
                    }
                "},
                indoc! {"
                    // Some other function
                    fn bar() {
                        // Some code
                    }

                    #[derive(Debug)]
                    fn foo() {
                        // Old implementation
                    }

                    // Another function
                    fn baz() {
                        // Some code
                    }
                "},
                indoc! {"
                    // Some other function
                    fn bar() {
                        // Some code
                    }

                    #[derive(Debug, Clone)]
                    fn foo() {
                        println!(\"hello from new foo\")
                    }

                    // Another function
                    fn baz() {
                        // Some code
                    }
                "},
            ),
        ];

        for (name, path, text, input, expected_output) in test_cases {
            let smart = Smart {
                path: path.into(),
                text: text.trim().to_string(),
            };

            let result = smart
                .apply(input)
                .unwrap_or_else(|_| panic!("Failed to apply smart change: {}", name));
            assert_eq!(result, expected_output.trim_end(), "Test case: {}", name);
        }
    }
}
