//! Patch operations that modify state. View operations are also included here, which lets us
//! sequence them with other operations.
mod insert;
mod replace;
mod replace_fuzzy;
mod write;

pub use insert::*;
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

    /// Insert text at a specific line in a file.
    Insert(insert::Insert),

    /// View just enters the path as an affected file without modifying it.
    View(PathBuf),

    /// View a range withina a file. Offsets are 0-based and exclusive. A None end offset means
    /// the end of the file.
    ViewRange(PathBuf, usize, Option<usize>),

    /// Undo reverts a single file to its previous state. Note that this adds a new snapshot entry,
    /// so undoing twice gets you back to the original state.       
    Undo(PathBuf),
}

impl Change {
    /// Returns the name of the change type.
    pub fn name(&self) -> &str {
        match self {
            Change::Write(_) => "write",
            Change::ReplaceFuzzy(_) => "replace_fuzzy",
            Change::Replace(_) => "replace",
            Change::Insert(_) => "insert",
            Change::View(_) => "view",
            Change::ViewRange(_, _, _) => "view_range",
            Change::Undo(_) => "undo",
        }
    }

    /// Returns the path of the file affected by this change.
    pub fn path(&self) -> &PathBuf {
        match self {
            Change::Write(write_file) => &write_file.path,
            Change::ReplaceFuzzy(replace) => &replace.path,
            Change::Replace(replace) => &replace.path,
            Change::Insert(insert) => &insert.path,
            Change::View(path) => path,
            Change::ViewRange(path, _, _) => path,
            Change::Undo(path) => path,
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
            Change::Insert(insert) => {
                let path_str = insert.path.to_string_lossy();
                renderer.push("insert");
                renderer.push(&format!(
                    "insert at line {} in file: {}",
                    insert.line, path_str
                ));
                renderer.push("content:");
                renderer.para(&insert.new);
                renderer.pop();
                renderer.pop();
                renderer.pop();
            }
            Change::View(_) => {
                renderer.para("view");
            }
            Change::Undo(path) => {
                let path_str = path.to_string_lossy();
                renderer.push("undo");
                renderer.para(&format!("undo changes to: {}", path_str));
                renderer.pop();
            }
            Change::ViewRange(path, start, end) => {
                let path_str = path.to_string_lossy();
                renderer.push("view_range");
                let end_str = match end {
                    Some(e) => e.to_string(),
                    None => "end".to_string(),
                };
                renderer.para(&format!(
                    "view range from {} to {} in file: {}",
                    start, end_str, path_str
                ));
                renderer.pop();
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
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Adds a WriteFile change to the patch
    pub fn with_write<P, S>(mut self, path: P, content: S) -> Self
    where
        P: AsRef<std::path::Path>,
        S: AsRef<str>,
    {
        self.changes.push(Change::Write(WriteFile {
            path: path.as_ref().to_path_buf(),
            content: content.as_ref().to_string(),
        }));
        self
    }

    /// Adds a ReplaceFuzzy change to the patch
    pub fn with_replace_fuzzy<P, S1, S2>(mut self, path: P, old: S1, new: S2) -> Self
    where
        P: AsRef<std::path::Path>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        self.changes.push(Change::ReplaceFuzzy(ReplaceFuzzy {
            path: path.as_ref().to_path_buf(),
            old: old.as_ref().to_string(),
            new: new.as_ref().to_string(),
        }));
        self
    }

    /// Adds a Replace change to the patch
    pub fn with_replace<P, S1, S2>(mut self, path: P, old: S1, new: S2) -> Self
    where
        P: AsRef<std::path::Path>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        self.changes.push(Change::Replace(Replace {
            path: path.as_ref().to_path_buf(),
            old: old.as_ref().to_string(),
            new: new.as_ref().to_string(),
        }));
        self
    }

    /// Adds an Insert change to the patch
    pub fn with_insert<P, S>(mut self, path: P, line: usize, content: S) -> Self
    where
        P: AsRef<std::path::Path>,
        S: AsRef<str>,
    {
        self.changes.push(Change::Insert(Insert {
            path: path.as_ref().to_path_buf(),
            line,
            new: content.as_ref().to_string(),
        }));
        self
    }

    /// Adds a Touch change to the patch
    pub fn with_view<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.changes.push(Change::View(path.as_ref().to_path_buf()));
        self
    }

    /// Adds an Undo change to the patch
    pub fn with_undo<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.changes.push(Change::Undo(path.as_ref().to_path_buf()));
        self
    }

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
        let patch = Patch::default()
            .with_write("file1.txt", "content")
            .with_replace_fuzzy("file2.txt", "old", "new");

        let changed_files = patch.affected_files();
        assert_eq!(changed_files.len(), 2);
        assert!(changed_files.contains(&PathBuf::from("file1.txt")));
        assert!(changed_files.contains(&PathBuf::from("file2.txt")));
    }

    #[test]
    fn test_convenience_constructors() {
        let patch = Patch::default()
            .with_write("file1.txt", "content")
            .with_replace_fuzzy("file2.txt", "old", "new")
            .with_replace("file3.txt", "old", "new")
            .with_insert("file6.txt", 3, "inserted content")
            .with_view("file4.txt")
            .with_undo("file5.txt");

        assert_eq!(patch.changes.len(), 6);

        let changed_files = patch.affected_files();
        assert_eq!(changed_files.len(), 6);
        assert!(changed_files.contains(&PathBuf::from("file1.txt")));
        assert!(changed_files.contains(&PathBuf::from("file2.txt")));
        assert!(changed_files.contains(&PathBuf::from("file3.txt")));
        assert!(changed_files.contains(&PathBuf::from("file4.txt")));
        assert!(changed_files.contains(&PathBuf::from("file5.txt")));
        assert!(changed_files.contains(&PathBuf::from("file6.txt")));
    }
}
