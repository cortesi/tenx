mod replace;
mod smart;
mod udiff;
mod write;

pub use replace::*;
pub use smart::*;
pub use udiff::*;
pub use write::*;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// A change to be applied to a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Change {
    Write(write::WriteFile),
    Replace(replace::Replace),
    Smart(smart::Smart),
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
                Change::Smart(block) => block.path.clone(),
            })
            .collect()
    }

    /// Returns a string representation of the change for display purposes.
    pub fn change_description(change: &Change) -> String {
        match change {
            Change::Write(write_file) => format!("Write to {}", write_file.path.display()),
            Change::Replace(replace) => format!("Replace in {}", replace.path.display()),
            Change::Smart(block) => format!("Smart in {}", block.path.display()),
        }
    }

    /// Applies all changes in the patch to the provided cache.
    pub fn apply(&self, cache: &mut HashMap<PathBuf, String>) -> Result<()> {
        for change in &self.changes {
            match change {
                Change::Replace(replace) => replace.apply_to_cache(cache)?,
                Change::Write(write_file) => write_file.apply_to_cache(cache)?,
                Change::Smart(smart) => smart.apply_to_cache(cache)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_changed_files() {
        let mut patch = Patch::default();
        patch.changes.push(Change::Write(write::WriteFile {
            path: PathBuf::from("file1.txt"),
            content: "content".to_string(),
        }));
        patch.changes.push(Change::Replace(replace::Replace {
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
    fn test_apply() {
        let mut patch = Patch::default();
        patch.changes.push(Change::Write(write::WriteFile {
            path: PathBuf::from("file1.txt"),
            content: "new content".to_string(),
        }));
        patch.changes.push(Change::Replace(replace::Replace {
            path: PathBuf::from("file2.txt"),
            old: "content with old text".to_string(),
            new: "content with new text".to_string(),
        }));

        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("file1.txt"), "initial content".to_string());
        cache.insert(
            PathBuf::from("file2.txt"),
            "content with old text".to_string(),
        );

        patch.apply(&mut cache).unwrap();

        assert_eq!(
            cache.get(&PathBuf::from("file1.txt")).unwrap(),
            "new content"
        );
        assert_eq!(
            cache.get(&PathBuf::from("file2.txt")).unwrap(),
            "content with new text"
        );
    }
}
