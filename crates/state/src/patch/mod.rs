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

/// Internal error type for patch operations
#[derive(Debug)]
pub(crate) struct PatchError {
    /// The user-facing error message
    pub user: String,
    /// The model-facing error message (for AI context)
    pub model: String,
}

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use unirend::{Detail, Render};

use crate::error::Result;

/// An operation on the state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Operation {
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

impl Operation {
    /// Returns the name of the operation type.
    pub fn name(&self) -> &str {
        match self {
            Operation::Write(_) => "write",
            Operation::ReplaceFuzzy(_) => "replace_fuzzy",
            Operation::Replace(_) => "replace",
            Operation::Insert(_) => "insert",
            Operation::View(_) => "view",
            Operation::ViewRange(_, _, _) => "view_range",
            Operation::Undo(_) => "undo",
        }
    }

    /// Returns the path of the file affected by this operation.
    pub fn path(&self) -> &PathBuf {
        match self {
            Operation::Write(write_file) => &write_file.path,
            Operation::ReplaceFuzzy(replace) => &replace.path,
            Operation::Replace(replace) => &replace.path,
            Operation::Insert(insert) => &insert.path,
            Operation::View(path) => path,
            Operation::ViewRange(path, _, _) => path,
            Operation::Undo(path) => path,
        }
    }

    /// Returns true if this operation modifies the state, false if it's read-only.
    /// View and ViewRange operations are not considered modifications.
    pub fn is_modification(&self) -> bool {
        match self {
            Operation::Write(_) => true,
            Operation::ReplaceFuzzy(_) => true,
            Operation::Replace(_) => true,
            Operation::Insert(_) => true,
            Operation::View(_) => false,
            Operation::ViewRange(_, _, _) => false,
            Operation::Undo(_) => true,
        }
    }

    /// Is this operation a view operation?
    pub fn is_view(&self) -> bool {
        matches!(self, Operation::View(_) | Operation::ViewRange(_, _, _))
    }

    /// Renders this operation with the specified level of detail
    pub fn render<R: Render>(&self, renderer: &mut R, _detail: Detail) -> Result<()> {
        match self {
            Operation::Write(write_file) => {
                let path_str = write_file.path.to_string_lossy();
                renderer.push("write");
                renderer.push(&format!("write: {path_str}"));
                renderer.para(&write_file.content);
                renderer.pop();
                renderer.pop();
            }
            Operation::ReplaceFuzzy(replace) => {
                let path_str = replace.path.to_string_lossy();
                renderer.push("replace_fuzzy");
                renderer.push(&format!("replace (fuzzy) in file: {path_str}"));
                renderer.push("old:");
                renderer.para(&replace.old);
                renderer.pop();
                renderer.push("new:");
                renderer.para(&replace.new);
                renderer.pop();
                renderer.pop();
                renderer.pop();
            }
            Operation::Replace(replace) => {
                let path_str = replace.path.to_string_lossy();
                renderer.push("replace");
                renderer.push(&format!("replace (exact) in file: {path_str}"));
                renderer.push("old:");
                renderer.para(&replace.old);
                renderer.pop();
                renderer.push("new:");
                renderer.para(&replace.new);
                renderer.pop();
                renderer.pop();
                renderer.pop();
            }
            Operation::Insert(insert) => {
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
            Operation::View(_) => {
                renderer.para("view");
            }
            Operation::Undo(path) => {
                let path_str = path.to_string_lossy();
                renderer.push("undo");
                renderer.para(&format!("undo ops to: {path_str}"));
                renderer.pop();
            }
            Operation::ViewRange(path, start, end) => {
                let path_str = path.to_string_lossy();
                renderer.push("view_range");
                let end_str = match end {
                    Some(e) => e.to_string(),
                    None => "end".to_string(),
                };
                renderer.para(&format!(
                    "view range from {start} to {end_str} in file: {path_str}",
                ));
                renderer.pop();
            }
        }
        Ok(())
    }
}

/// A unified collection of operations, to be applied as a single pass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Patch {
    /// A list of operations
    pub ops: Vec<Operation>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Adds a WriteFile operation to the patch
    pub fn with_write<P, S>(mut self, path: P, content: S) -> Self
    where
        P: AsRef<std::path::Path>,
        S: AsRef<str>,
    {
        self.ops.push(Operation::Write(WriteFile {
            path: path.as_ref().to_path_buf(),
            content: content.as_ref().to_string(),
        }));
        self
    }

    /// Adds a ReplaceFuzzy operation to the patch
    pub fn with_replace_fuzzy<P, S1, S2>(mut self, path: P, old: S1, new: S2) -> Self
    where
        P: AsRef<std::path::Path>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        self.ops.push(Operation::ReplaceFuzzy(ReplaceFuzzy {
            path: path.as_ref().to_path_buf(),
            old: old.as_ref().to_string(),
            new: new.as_ref().to_string(),
        }));
        self
    }

    /// Adds a Replace operation to the patch
    pub fn with_replace<P, S1, S2>(mut self, path: P, old: S1, new: S2) -> Self
    where
        P: AsRef<std::path::Path>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        self.ops.push(Operation::Replace(Replace {
            path: path.as_ref().to_path_buf(),
            old: old.as_ref().to_string(),
            new: new.as_ref().to_string(),
        }));
        self
    }

    /// Adds an Insert operation to the patch
    pub fn with_insert<P, S>(mut self, path: P, line: usize, content: S) -> Self
    where
        P: AsRef<std::path::Path>,
        S: AsRef<str>,
    {
        self.ops.push(Operation::Insert(Insert {
            path: path.as_ref().to_path_buf(),
            line,
            new: content.as_ref().to_string(),
        }));
        self
    }

    /// Adds a Touch operation to the patch
    pub fn with_view<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.ops.push(Operation::View(path.as_ref().to_path_buf()));
        self
    }

    /// Adds a ViewRange operation to the patch
    pub fn with_view_range<P: AsRef<std::path::Path>>(
        mut self,
        path: P,
        start: usize,
        end: Option<usize>,
    ) -> Self {
        self.ops.push(Operation::ViewRange(
            path.as_ref().to_path_buf(),
            start,
            end,
        ));
        self
    }

    /// Adds a ViewRange operation to the patch with one-based indexing
    ///
    /// Takes start and end line numbers in one-based form.
    /// If end is -1, it's considered to be the end of the file.
    pub fn with_view_range_onebased<P: AsRef<std::path::Path>>(
        self,
        path: P,
        start: isize,
        end: isize,
    ) -> Self {
        let start_zero_based = if start > 0 { start - 1 } else { 0 };
        let end_opt = if end == -1 {
            None
        } else if end > 0 {
            Some(end as usize)
        } else {
            Some(0)
        };

        self.with_view_range(path, start_zero_based as usize, end_opt)
    }

    /// Adds an Undo operation to the patch
    pub fn with_undo<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.ops.push(Operation::Undo(path.as_ref().to_path_buf()));
        self
    }

    /// Returns a vector of unique PathBufs for all files affected by the patch.
    pub fn affected_files(&self) -> Vec<PathBuf> {
        let mut paths = HashMap::new();
        for op in &self.ops {
            paths.insert(op.path().clone(), ());
        }
        paths.into_keys().collect()
    }

    /// Groups ops by file path
    fn ops_by_file(&self) -> HashMap<&PathBuf, Vec<&Operation>> {
        let mut file_ops = HashMap::new();
        for op in &self.ops {
            file_ops.entry(op.path()).or_insert_with(Vec::new).push(op);
        }
        file_ops
    }

    /// Renders the patch with the specified level of detail
    pub fn render<R: Render>(&self, renderer: &mut R, detail: Detail) -> Result<()> {
        let affected_files = self.affected_files();

        // Simplest summary for minimal detail
        if detail < Detail::Default {
            renderer.para(&format!(
                "{} opserations on {} files",
                self.ops.len(),
                affected_files.len()
            ));
        } else {
            let file_ops = self.ops_by_file();
            for (file, ops) in file_ops {
                renderer.push(&file.to_string_lossy());
                if detail >= Detail::Detailed {
                    for op in ops {
                        op.render(renderer, detail)?;
                    }
                } else {
                    let mut counts: HashMap<String, usize> = HashMap::new();
                    for c in ops {
                        let count = counts.entry(c.name().to_string()).or_insert(0);
                        *count += 1;
                    }
                    let counts: Vec<String> = counts
                        .iter()
                        .map(|(name, count)| format!("{name} ({count})"))
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
    fn test_affected_files() {
        let patch = Patch::default()
            .with_write("file1.txt", "content")
            .with_replace_fuzzy("file2.txt", "old", "new");

        let affected_files = patch.affected_files();
        assert_eq!(affected_files.len(), 2);
        assert!(affected_files.contains(&PathBuf::from("file1.txt")));
        assert!(affected_files.contains(&PathBuf::from("file2.txt")));
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

        assert_eq!(patch.ops.len(), 6);

        let affected_files = patch.affected_files();
        assert_eq!(affected_files.len(), 6);
        assert!(affected_files.contains(&PathBuf::from("file1.txt")));
        assert!(affected_files.contains(&PathBuf::from("file2.txt")));
        assert!(affected_files.contains(&PathBuf::from("file3.txt")));
        assert!(affected_files.contains(&PathBuf::from("file4.txt")));
        assert!(affected_files.contains(&PathBuf::from("file5.txt")));
        assert!(affected_files.contains(&PathBuf::from("file6.txt")));
    }

    #[test]
    fn test_is_modification() {
        // Test modifying operations
        assert!(Operation::Write(write::WriteFile {
            path: PathBuf::from("test.txt"),
            content: "content".to_string(),
        })
        .is_modification());

        assert!(Operation::ReplaceFuzzy(replace_fuzzy::ReplaceFuzzy {
            path: PathBuf::from("test.txt"),
            old: "old".to_string(),
            new: "new".to_string(),
        })
        .is_modification());

        assert!(Operation::Replace(replace::Replace {
            path: PathBuf::from("test.txt"),
            old: "old".to_string(),
            new: "new".to_string(),
        })
        .is_modification());

        assert!(Operation::Insert(insert::Insert {
            path: PathBuf::from("test.txt"),
            line: 0,
            new: "content".to_string(),
        })
        .is_modification());

        assert!(Operation::Undo(PathBuf::from("test.txt")).is_modification());

        // Test non-modifying operations
        assert!(!Operation::View(PathBuf::from("test.txt")).is_modification());
        assert!(!Operation::ViewRange(PathBuf::from("test.txt"), 0, Some(10)).is_modification());
    }
}
