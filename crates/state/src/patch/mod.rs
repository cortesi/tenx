//! Patch operations that modify state.
mod replace;
mod replace_fuzzy;
mod write;

pub use replace::*;
pub use replace_fuzzy::*;
pub use write::*;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use unirend::{Detail, Render};

use crate::error::Result;

/// A change to be applied to the state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Change {
    /// Write or create a complete file.
    Write(write::WriteFile),

    /// Replace one piece of text with another, with fuzzy matching.
    ReplaceFuzzy(replace_fuzzy::ReplaceFuzzy),

    /// Replace one piece of text with another, requiring an exact match.
    Replace(replace::Replace),

    /// Touch is a NoOp, it just enters the path as an affected file without modifying it.
    Touch(PathBuf),
}

impl Change {
    /// Returns the name of the change type.
    pub fn name(&self) -> &str {
        match self {
            Change::Write(_) => "write",
            Change::ReplaceFuzzy(_) => "replace_fuzzy",
            Change::Replace(_) => "replace",
            Change::Touch(_) => "view",
        }
    }

    /// Returns the path of the file affected by this change.
    pub fn path(&self) -> &PathBuf {
        match self {
            Change::Write(write_file) => &write_file.path,
            Change::ReplaceFuzzy(replace) => &replace.path,
            Change::Replace(replace) => &replace.path,
            Change::Touch(path) => path,
        }
    }

    /// Applies this change to the input string, returning the modified string.
    pub fn apply(&self, input: &str) -> Result<String> {
        match self {
            Change::Write(write_file) => Ok(write_file.content.clone()),
            Change::ReplaceFuzzy(replace) => replace.apply(input),
            Change::Replace(replace) => replace.apply(input),
            Change::Touch(_) => Ok(input.to_string()),
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
            Change::ReplaceFuzzy(replace) => {
                let path_str = replace.path.to_string_lossy();
                renderer.push("replace_fuzzy");
                renderer.push(&format!("replace (fuzzy) in file: {}", path_str));
                renderer.push("old:");
                renderer.para(&replace.old);
                renderer.pop();
                renderer.push("new:");
                renderer.para(&replace.new);
                renderer.pop();
                renderer.pop();
                renderer.pop();
            }
            Change::Replace(replace) => {
                let path_str = replace.path.to_string_lossy();
                renderer.push("replace");
                renderer.push(&format!("replace (exact) in file: {}", path_str));
                renderer.push("old:");
                renderer.para(&replace.old);
                renderer.pop();
                renderer.push("new:");
                renderer.para(&replace.new);
                renderer.pop();
                renderer.pop();
                renderer.pop();
            }
            Change::Touch(_) => {
                renderer.para("view");
            }
        }
        Ok(())
    }
}

/// A unified collection of Change operations, to be applied as a single patch.
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
        patch
            .changes
            .push(Change::ReplaceFuzzy(replace_fuzzy::ReplaceFuzzy {
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

