//! Patch operations that modify files in the project.
mod replace;
mod write;

pub use replace::*;
pub use write::*;

use std::collections::HashMap;
use std::path::PathBuf;

use fs_err;
use serde::{Deserialize, Serialize};
use unirend::{Detail, Render};

use crate::{config::Config, error::Result};

/// A change to be applied to the state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Change {
    /// Wrhite a complete file.
    Write(write::WriteFile),
    /// Replace one piece of text with another.
    Replace(replace::Replace),
    /// View is basically a NoOp, it just enters the path as an affected file, which in turn
    /// causes it to be output by the dialect at the appropriate point.
    View(PathBuf),
}

impl Change {
    pub fn name(&self) -> &str {
        match self {
            Change::Write(_) => "write",
            Change::Replace(_) => "replace",
            Change::View(_) => "view",
        }
    }

    pub fn path(&self) -> &PathBuf {
        match self {
            Change::Write(write_file) => &write_file.path,
            Change::Replace(replace) => &replace.path,
            Change::View(path) => path,
        }
    }

    pub fn apply(&self, input: &str) -> Result<String> {
        match self {
            Change::Write(write_file) => Ok(write_file.content.clone()),
            Change::Replace(replace) => replace.apply(input),
            Change::View(_) => Ok(input.to_string()),
        }
    }

    /// Renders this change with the specified level of detail
    pub fn render<R: Render>(&self, renderer: &mut R, _detail: Detail) -> Result<()> {
        match self {
            Change::Write(write_file) => {
                let path_str = write_file.path.to_string_lossy();
                renderer.push("write");
                renderer.push(&format!("write: {}", path_str));
                renderer.para(&write_file.content);
                renderer.pop();
                renderer.pop();
            }
            Change::Replace(replace) => {
                let path_str = replace.path.to_string_lossy();
                renderer.push("replace");
                renderer.push(&format!("replace in file: {}", path_str));
                renderer.push("old:");
                renderer.para(&replace.old);
                renderer.pop();
                renderer.push("new:");
                renderer.para(&replace.new);
                renderer.pop();
                renderer.pop();
                renderer.pop();
            }
            Change::View(_) => {
                renderer.para("view");
            }
        }
        Ok(())
    }
}

/// A unified patch operation requested by the model. This contains all changes, as well as a cache
/// of file state before the patch is applied, so we can roll back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Patch {
    pub changes: Vec<Change>,
}

impl Patch {
    /// Returns a vector of unique PathBufs for all files changed in the patch.
    pub fn affected_files(&self) -> Vec<PathBuf> {
        let mut paths = HashMap::new();
        for change in &self.changes {
            paths.insert(change.path().clone(), ());
        }
        paths.into_keys().collect()
    }

    /// Takes a snapshot of the current state of all files that would be modified by this patch.
    pub fn snapshot(&self, config: &Config) -> Result<HashMap<PathBuf, String>> {
        let mut snapshot = HashMap::new();
        for path in self.affected_files() {
            let abs_path = config.abspath(&path)?;
            let content = fs_err::read_to_string(&abs_path)?;
            snapshot.insert(path, content);
        }
        Ok(snapshot)
    }

    /// Groups changes by file path
    fn changes_by_file(&self) -> HashMap<&PathBuf, Vec<&Change>> {
        let mut file_changes = HashMap::new();
        for change in &self.changes {
            file_changes
                .entry(change.path())
                .or_insert_with(Vec::new)
                .push(change);
        }
        file_changes
    }

    /// Renders the patch with the specified level of detail
    pub fn render<R: Render>(&self, renderer: &mut R, detail: Detail) -> Result<()> {
        let affected_files = self.affected_files();

        // Simplest summary for minimal detail
        if detail < Detail::Default {
            renderer.para(&format!(
                "{} changes made to {} files",
                self.changes.len(),
                affected_files.len()
            ));
        } else {
            let file_changes = self.changes_by_file();
            for (file, changes) in file_changes {
                renderer.push(&file.to_string_lossy());
                if detail >= Detail::Detailed {
                    for change in changes {
                        change.render(renderer, detail)?;
                    }
                } else {
                    let mut counts: HashMap<String, usize> = HashMap::new();
                    for c in changes {
                        let count = counts.entry(c.name().to_string()).or_insert(0);
                        *count += 1;
                    }
                    let counts: Vec<String> = counts
                        .iter()
                        .map(|(name, count)| format!("{} ({})", name, count))
                        .collect();
                    renderer.para(&counts.join(", "));
                }

                renderer.pop();
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

        let changed_files = patch.affected_files();
        assert_eq!(changed_files.len(), 2);
        assert!(changed_files.contains(&PathBuf::from("file1.txt")));
        assert!(changed_files.contains(&PathBuf::from("file2.txt")));
    }
}
