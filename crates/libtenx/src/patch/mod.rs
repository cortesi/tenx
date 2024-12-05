//! Patch operations that modify files in the project.
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

use fs_err;
use serde::{Deserialize, Serialize};

use crate::{config::Config, error::Result};

/// A change to be applied to a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Change {
    Write(write::WriteFile),
    Replace(replace::Replace),
    Smart(smart::Smart),
    UDiff(udiff::UDiff),
}

/// A unified patch operation requested by the model. This contains all changes, as well as a cache
/// of file state before the patch is applied, so we can roll back.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Patch {
    pub changes: Vec<Change>,
}

impl Patch {
    /// Returns a vector of PathBufs for all files changed in the patch.
    pub fn changed_files(&self) -> Vec<PathBuf> {
        let mut paths = vec![];
        for change in &self.changes {
            match change {
                Change::Write(write_file) => paths.push(write_file.path.clone()),
                Change::Replace(replace) => paths.push(replace.path.clone()),
                Change::Smart(block) => paths.push(block.path.clone()),
                Change::UDiff(udiff) => paths.extend(udiff.modified_files.iter().map(|f| f.into())),
            }
        }
        paths
    }

    /// Takes a snapshot of the current state of all files that would be modified by this patch.
    pub fn snapshot(&self, config: &Config) -> Result<HashMap<PathBuf, String>> {
        let mut snapshot = HashMap::new();
        for path in self.changed_files() {
            let abs_path = config.abspath(&path)?;
            let content = fs_err::read_to_string(&abs_path)?;
            snapshot.insert(path, content);
        }
        Ok(snapshot)
    }

    /// Applies all changes in the patch, updating both the cache and the filesystem.
    pub(crate) fn apply(&self, config: &Config) -> Result<()> {
        // Next, make a clone copy of the cache
        let mut modified_cache = self.snapshot(config)?;

        // Apply all modifications to the cloned cache
        for change in &self.changes {
            match change {
                Change::Replace(replace) => replace.apply_to_cache(&mut modified_cache)?,
                Change::Write(write_file) => write_file.apply_to_cache(&mut modified_cache)?,
                Change::Smart(smart) => smart.apply_to_cache(&mut modified_cache)?,
                Change::UDiff(udiff) => udiff.apply_to_cache(&mut modified_cache)?,
            }
        }

        // Finally, write all files to disk
        for (path, content) in modified_cache {
            let abs_path = config.abspath(&path)?;
            fs_err::write(&abs_path, content)?;
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
        use crate::testutils::test_project;
        let test_project = test_project();
        test_project.create_file_tree(&["file1.txt", "file2.txt"]);
        test_project.write("file1.txt", "initial content");
        test_project.write("file2.txt", "content with old text");

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

        patch.apply(&test_project.config).unwrap();

        assert_eq!(test_project.read("file1.txt"), "new content");
        assert_eq!(test_project.read("file2.txt"), "content with new text");
    }
}
