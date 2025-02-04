#![doc = "This is an experimental feature, not ready for use!"]

use crate::error::{Result, TenxError};
use fudiff::{parse, FuDiff};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Represents a unified diff based on FuDiff from the fudiff library.
/// The filename is provided externally (via an xml tag) since the FuDiff format does not include it.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct UDiff {
    pub path: String,
    pub fudiff: FuDiff,
}

impl UDiff {
    /// Creates a new UDiff instance by parsing the provided diff text using FuDiff.
    /// The filename must be provided separately.
    pub fn new(file: String, patch: String) -> Result<Self> {
        if patch
            .lines()
            .filter(|l| l.trim_start().starts_with("--- "))
            .count()
            > 1
        {
            return Err(TenxError::Patch {
                user: "Multi-file diffs are not supported".to_string(),
                model: "The provided diff contains multiple file sections".to_string(),
            });
        }
        let fudiff = parse(&patch).map_err(|e| TenxError::Patch {
            user: "Failed to parse unified diff".to_string(),
            model: format!("Error parsing diff: {:?}", e),
        })?;
        Ok(UDiff { path: file, fudiff })
    }

    /// Applies the unified diff (via FuDiff) to the given file content in the cache.
    pub fn apply_to_cache(&self, cache: &mut HashMap<PathBuf, String>) -> Result<()> {
        let path = PathBuf::from(&self.path);
        let current_content = cache.get(&path).ok_or_else(|| TenxError::Patch {
            user: format!("File not found in cache: {}", self.path),
            model: format!("File {} is not present in the cache", self.path),
        })?;

        let new_content = self
            .fudiff
            .patch(current_content)
            .map_err(|e| TenxError::Patch {
                user: format!("Failed to apply patch to file: {}", self.path),
                model: format!("Error applying patch to {}: {:?}", self.path, e),
            })?;

        cache.insert(path, new_content);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_valid_single_file_udiff() {
        let valid_diff = indoc! {"
            @@  @@
             unchanged line
            -removed line
            +added line
        "};
        // Using the external file name "file.txt"
        let udiff = UDiff::new("file.txt".to_string(), valid_diff.to_string()).unwrap();
        assert_eq!(udiff.path, "file.txt");
    }

    #[test]
    fn test_invalid_udiff() {
        let invalid_diff = indoc! {"
            invalid line
            @@  @@
             unchanged line
            -removed line
            +added line
        "};
        let result = UDiff::new("file.txt".to_string(), invalid_diff.to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_to_cache() {
        let valid_diff = indoc! {"
            @@  @@
             unchanged line
            -removed line
            +added line
             last line
        "};
        let udiff = UDiff::new("file.txt".to_string(), valid_diff.to_string()).unwrap();
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
    fn test_apply_to_cache_no_trailing_newline() {
        let valid_diff = indoc! {"
            @@  @@
             unchanged line
            -removed line
            +added line
             last line
        "};
        let udiff = UDiff::new("file.txt".to_string(), valid_diff.to_string()).unwrap();
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

