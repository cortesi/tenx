use crate::error::{Result, TenxError};
use diffy::{apply, Patch as DiffyPatch};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Represents a unified diff, containing the full patch and a list of modified files.
///
/// This struct stores the entire unified diff as a string and keeps track of all files
/// that are modified by the diff. It provides methods to create a new UDiff instance
/// with validation and to access the list of modified files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UDiff {
    pub patch: String,
    pub modified_files: Vec<String>,
}

impl UDiff {
    /// Creates a new UDiff instance after validating the input string as a unified diff.
    pub fn new(patch: String) -> Result<Self> {
        let modified_files = Self::validate_udiff(&patch)?;
        Ok(UDiff {
            patch,
            modified_files,
        })
    }

    fn validate_udiff(patch: &str) -> Result<Vec<String>> {
        let lines: Vec<&str> = patch.lines().collect();
        let mut modified_files = Vec::new();

        if lines.is_empty() {
            return Err(TenxError::Patch {
                user: "Invalid unified diff format".to_string(),
                model: "Unified diff is empty".to_string(),
            });
        }

        let mut i = 0;
        while i < lines.len() {
            // Skip git diff headers
            if lines[i].starts_with("diff --git") {
                i += 1;
                continue;
            }

            // Check for file header
            if !lines[i].starts_with("--- ")
                || i + 1 >= lines.len()
                || !lines[i + 1].starts_with("+++ ")
            {
                return Err(TenxError::Patch {
                    user: "Invalid unified diff format".to_string(),
                    model: "Each file diff must start with '--- ' and '+++ ' lines".to_string(),
                });
            }

            // Extract and store the modified file name
            if let Some(file_name) = lines[i + 1].strip_prefix("+++ b/") {
                modified_files.push(file_name.to_string());
            }

            i += 2;

            // Process hunks for this file
            while i < lines.len()
                && !lines[i].starts_with("--- ")
                && !lines[i].starts_with("diff --git")
            {
                if lines[i].starts_with("@@") {
                    if !lines[i].contains("@@") {
                        return Err(TenxError::Patch {
                            user: "Invalid unified diff format".to_string(),
                            model: "Hunk header is malformed".to_string(),
                        });
                    }
                    i += 1;
                    // Process hunk content
                    while i < lines.len()
                        && !lines[i].starts_with("@@")
                        && !lines[i].starts_with("--- ")
                        && !lines[i].starts_with("diff --git")
                    {
                        if !lines[i].is_empty() && !lines[i].starts_with([' ', '+', '-']) {
                            return Err(TenxError::Patch {
                                user: "Invalid unified diff format".to_string(),
                                model: "Unexpected line prefix in diff content".to_string(),
                            });
                        }
                        i += 1;
                    }
                } else {
                    return Err(TenxError::Patch {
                        user: "Invalid unified diff format".to_string(),
                        model: "Expected hunk header starting with '@@'".to_string(),
                    });
                }
            }
        }

        Ok(modified_files)
    }

    /// Returns a list of files modified by this diff.
    pub fn modified_files(&self) -> &[String] {
        &self.modified_files
    }

    /// Applies the unified diff to the given file content in the cache.
    pub fn apply_to_cache(&self, cache: &mut HashMap<PathBuf, String>) -> Result<()> {
        let patches = self.split_patches();

        for patch in patches {
            let diffy_patch = DiffyPatch::from_str(&patch).map_err(|e| TenxError::Patch {
                user: "Failed to parse unified diff".to_string(),
                model: format!("Error parsing unified diff: {}", e),
            })?;

            let file = self.extract_file_name(&patch)?;
            let path = PathBuf::from(file);
            let current_content = cache.get(&path).ok_or_else(|| TenxError::Patch {
                user: format!("File not found in cache: {}", file),
                model: format!("File {} is not present in the cache", file),
            })?;

            // Ensure the content ends with a newline
            let current_content_with_newline = if current_content.ends_with('\n') {
                current_content.clone()
            } else {
                format!("{}\n", current_content)
            };

            let new_content = apply(&current_content_with_newline, &diffy_patch).map_err(|e| {
                TenxError::Patch {
                    user: format!("Failed to apply patch to file: {}", file),
                    model: format!("Error applying patch to {}: {}", file, e),
                }
            })?;

            // Remove trailing newline if it wasn't present in the original content
            let new_content = if current_content.ends_with('\n') {
                new_content
            } else {
                new_content.trim_end_matches('\n').to_string()
            };

            cache.insert(path, new_content);
        }

        Ok(())
    }

    fn split_patches(&self) -> Vec<String> {
        if self.patch.starts_with("diff --git") {
            self.patch
                .split("diff --git")
                .skip(1)
                .map(|s| format!("diff --git{}", s))
                .collect()
        } else {
            vec![self.patch.clone()]
        }
    }

    fn extract_file_name<'a>(&self, patch: &'a str) -> Result<&'a str> {
        patch
            .lines()
            .find(|line| line.starts_with("+++ b/"))
            .and_then(|line| line.strip_prefix("+++ b/"))
            .ok_or_else(move || TenxError::Patch {
                user: "Invalid patch format".to_string(),
                model: "Could not find target file name in patch".to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_valid_single_file_udiff() {
        let valid_diff = indoc! {"
            --- a/file.txt
            +++ b/file.txt
            @@ -1,3 +1,3 @@
             unchanged line
            -removed line
            +added line
        "};

        let udiff = UDiff::new(valid_diff.to_string()).unwrap();
        assert_eq!(udiff.modified_files(), &["file.txt"]);
    }

    #[test]
    fn test_valid_multi_file_udiff() {
        let valid_diff = indoc! {"
            --- a/file1.txt
            +++ b/file1.txt
            @@ -1,3 +1,3 @@
             unchanged line
            -removed line
            +added line
            --- a/file2.txt
            +++ b/file2.txt
            @@ -1,2 +1,3 @@
             first line
            +inserted line
             last line
        "};

        let udiff = UDiff::new(valid_diff.to_string()).unwrap();
        assert_eq!(udiff.modified_files(), &["file1.txt", "file2.txt"]);
    }

    #[test]
    fn test_invalid_udiff() {
        let invalid_diff = indoc! {"
            --- a/file.txt
            +++ b/file.txt
            invalid line
            @@ -1,3 +1,3 @@
             unchanged line
            -removed line
            +added line
        "};

        assert!(UDiff::new(invalid_diff.to_string()).is_err());
    }

    #[test]
    fn test_invalid_multi_file_udiff() {
        let invalid_diff = indoc! {"
            --- a/file1.txt
            +++ b/file1.txt
            @@ -1,3 +1,3 @@
             unchanged line
            -removed line
            +added line
            --- a/file2.txt
            ++ b/file2.txt  // Invalid: missing a '+'
            @@ -1,2 +1,3 @@
             first line
            +inserted line
             last line
        "};

        assert!(UDiff::new(invalid_diff.to_string()).is_err());
    }

    #[test]
    fn test_apply_to_cache() {
        let valid_diff = indoc! {"
            --- a/file.txt
            +++ b/file.txt
            @@ -1,3 +1,3 @@
             unchanged line
            -removed line
            +added line
             last line
        "};

        let udiff = UDiff::new(valid_diff.to_string()).unwrap();
        let mut cache = HashMap::new();
        cache.insert(
            PathBuf::from("file.txt"),
            "unchanged line\nremoved line\nlast line\n".to_string(),
        );

        udiff.apply_to_cache(&mut cache).unwrap();

        assert_eq!(
            cache.get(&PathBuf::from("file.txt")).unwrap(),
            "unchanged line\nadded line\nlast line\n"
        );
    }

    #[test]
    fn test_apply_to_cache_multi_file() {
        let valid_diff = indoc! {"
            diff --git a/file1.txt b/file1.txt
            --- a/file1.txt
            +++ b/file1.txt
            @@ -1,3 +1,3 @@
             unchanged line
            -removed line
            +added line
             last line
            diff --git a/file2.txt b/file2.txt
            --- a/file2.txt
            +++ b/file2.txt
            @@ -1,2 +1,3 @@
             first line
            +inserted line
             last line
        "};

        let udiff = UDiff::new(valid_diff.to_string()).unwrap();
        let mut cache = HashMap::new();
        cache.insert(
            PathBuf::from("file1.txt"),
            "unchanged line\nremoved line\nlast line\n".to_string(),
        );
        cache.insert(
            PathBuf::from("file2.txt"),
            "first line\nlast line\n".to_string(),
        );

        udiff.apply_to_cache(&mut cache).unwrap();

        assert_eq!(
            cache.get(&PathBuf::from("file1.txt")).unwrap(),
            "unchanged line\nadded line\nlast line\n"
        );
        assert_eq!(
            cache.get(&PathBuf::from("file2.txt")).unwrap(),
            "first line\ninserted line\nlast line\n"
        );
    }

    #[test]
    fn test_apply_to_cache_no_trailing_newline() {
        let valid_diff = indoc! {"
            --- a/file.txt
            +++ b/file.txt
            @@ -1,3 +1,3 @@
             unchanged line
            -removed line
            +added line
             last line
        "};

        let udiff = UDiff::new(valid_diff.to_string()).unwrap();
        let mut cache = HashMap::new();
        cache.insert(
            PathBuf::from("file.txt"),
            "unchanged line\nremoved line\nlast line".to_string(),
        );

        udiff.apply_to_cache(&mut cache).unwrap();

        assert_eq!(
            cache.get(&PathBuf::from("file.txt")).unwrap(),
            "unchanged line\nadded line\nlast line"
        );
    }
}
