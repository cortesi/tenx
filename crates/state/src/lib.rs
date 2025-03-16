//! A unified interface over all persistent state.
//!
//! All modifications are made through `Patch` operations, and return an ID that can be used
//! to revert the state to a previous snapshot.
//!
//! The actual state consists of a filesystem directory and an in-memory store. Files in the
//! in-memory store are prefixed with `::`.
mod directory;
mod error;
mod memory;

pub mod abspath;
pub mod files;
mod patch;

pub use crate::error::*;
pub use crate::patch::*;

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt::Debug,
    path::{Path, PathBuf},
};

use globset::Glob;
use serde::{Deserialize, Serialize};

/// Prefix for in-memory files
pub const MEM_PREFIX: &str = "::";

trait SubStore: Debug {
    fn list(&self) -> Result<Vec<PathBuf>>;
    fn read(&self, path: &Path) -> Result<String>;
    fn write(&mut self, path: &Path, content: &str) -> Result<()>;
    fn remove(&mut self, path: &Path) -> Result<()>;
}

/// Information about a patch operation, including success/failure counts and any errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    pub rollback_id: u64,
    pub succeeded: usize,
    /// Some operations, like View, mean the dialogue with the model should continue
    pub should_continue: bool,
    /// All errors here are of type TenxError::Patch
    pub failures: Vec<(Change, Error)>,
}

impl PatchInfo {
    pub fn add_failure(&mut self, change: Change, error: Error) -> Result<()> {
        match error {
            Error::Patch { user, model } => {
                self.failures.push((change, Error::Patch { user, model }));
                Ok(())
            }
            _ => Err(error),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Snapshot {
    content: HashMap<PathBuf, String>,
    created: Vec<PathBuf>,
}

impl Snapshot {
    pub fn insert(&mut self, path: PathBuf, content: String) {
        self.content.insert(path, content);
    }

    pub fn create(&mut self, path: PathBuf) {
        self.content.insert(path.clone(), String::new());
        self.created.push(path);
    }

    /// Returns a unique list of all files touched in the snapshot.
    pub fn touched(&self) -> Vec<PathBuf> {
        let mut touched = BTreeSet::new();
        for path in self.content.keys() {
            touched.insert(path.clone());
        }
        for path in &self.created {
            touched.insert(path.clone());
        }
        touched.into_iter().collect()
    }
}

/// The state underlying a session. This is the set of resources that our models are editing. State
/// presents a unified interface over an optional filesystem directory and a memory store.
/// In-memory file names are prefixed with "::"
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    directory: Option<directory::Directory>,
    memory: memory::Memory,
    snapshots: Vec<(u64, Snapshot)>,
    next_snapshot_id: u64,
}

impl State {
    /// Generate a diff of changes made to a file since the first snapshot.
    ///
    /// If the file has changed significantly (more than 50% of lines), a single
    /// WriteFile operation will be used instead of multiple Replace operations.
    pub fn diff_path(&self, path: PathBuf) -> Result<Patch> {
        // Get original and current content
        let original_content = self.original(path.as_path()).unwrap_or_default();
        let current_content = self.read(path.as_path())?;

        // Generate a diff using diffy
        let diff = diffy::create_patch(&original_content, &current_content);

        let mut changes = Vec::new();
        let mut total_replaced_lines = 0;
        let total_lines = current_content.lines().count();

        // Convert diff hunks to Change::Replace operations
        for hunk in diff.hunks() {
            let mut old_content = String::new();
            let mut new_content = String::new();
            let mut hunk_replaced_lines = 0;

            for line in hunk.lines() {
                match line {
                    diffy::Line::Context(text) => {
                        old_content.push_str(text);
                        new_content.push_str(text);
                    }
                    diffy::Line::Delete(text) => {
                        old_content.push_str(text);
                        hunk_replaced_lines += 1;
                    }
                    diffy::Line::Insert(text) => {
                        new_content.push_str(text);
                        hunk_replaced_lines += 1;
                    }
                }
            }

            total_replaced_lines += hunk_replaced_lines;

            changes.push(Change::Replace(patch::Replace {
                path: path.clone(),
                old: old_content,
                new: new_content,
            }));
        }

        // If more than half the file has changed, or if file is being completely emptied or filled,
        // use a single Write operation instead
        if total_lines == 0
            || original_content.is_empty()
            || current_content.is_empty()
            || (total_lines > 0 && (total_replaced_lines as f64) / (total_lines as f64) > 0.5)
        {
            changes = vec![Change::Write(WriteFile {
                path,
                content: current_content,
            })];
        }

        Ok(Patch { changes })
    }

    /// Set the directory path and glob patterns for file operations.
    ///
    /// Glob patterns can be positive (equivalent to --include) or negative (prefixed with `!`,
    /// equivalent to --exclude). If no glob patterns are provided, all files are included.
    pub fn with_directory<P>(mut self, root: P, globs: Vec<String>) -> Result<Self>
    where
        P: abspath::IntoAbsPath,
    {
        let abs = root.into_abs_path()?;
        self.directory = Some(directory::Directory::new(abs, globs)?);
        Ok(self)
    }

    /// Initialize the state with pre-populated memory contents.
    ///
    /// This method takes a HashMap mapping file paths to their contents and
    /// adds these files to the memory store of the State.
    pub fn with_memory(mut self, files: HashMap<PathBuf, String>) -> Result<Self> {
        for (path, content) in files {
            self.memory.write(&path, &content)?;
        }
        Ok(self)
    }

    /// Dispatches an operation to the appropriate immutable store based on the path prefix.
    fn dispatch_ro<T, F>(&self, path: &Path, f: F) -> Result<T>
    where
        F: FnOnce(&dyn SubStore) -> Result<T>,
    {
        if path.to_string_lossy().starts_with(MEM_PREFIX) {
            f(&self.memory)
        } else if let Some(ref fs) = self.directory {
            f(fs)
        } else {
            Err(Error::NotFound {
                msg: "No matching store".to_string(),
                path: path.display().to_string(),
            })
        }
    }

    /// Dispatches an operation to the appropriate mutable store based on the path prefix.
    fn dispatch_mut<T, F>(&mut self, path: &Path, f: F) -> Result<T>
    where
        F: FnOnce(&mut dyn SubStore) -> Result<T>,
    {
        if path.to_string_lossy().starts_with(MEM_PREFIX) {
            f(&mut self.memory)
        } else if let Some(ref mut fs) = self.directory {
            f(fs)
        } else {
            Err(Error::NotFound {
                msg: "No matching store".to_string(),
                path: path.display().to_string(),
            })
        }
    }

    /// Retrieves the content associated with the given path.
    pub fn read(&self, path: &Path) -> Result<String> {
        self.dispatch_ro(path, |store| store.read(path))
    }

    /// Writes content to a path.
    fn write(&mut self, path: &Path, content: &str) -> Result<()> {
        self.dispatch_mut(path, |store| store.write(path, content))
    }

    /// Removes a file or memory entry for the given path.
    fn remove(&mut self, path: &Path) -> Result<()> {
        self.dispatch_mut(path, |store| store.remove(path))
    }

    /// Creates a snapshot of the given list of paths. For each path, if the file exists, its content is captured;
    /// otherwise, the path is marked as created.
    fn create_snapshot(&self, paths: &[PathBuf]) -> Result<Snapshot> {
        let mut snap = Snapshot::default();
        for p in paths {
            match self.read(p) {
                Ok(content) => snap.insert(p.clone(), content),
                Err(Error::NotFound { .. }) => snap.create(p.clone()),
                Err(e) => return Err(e),
            }
        }
        Ok(snap)
    }

    /// Reverts the state to the given snapshot.
    /// Restores content for files or memory entries that existed and removes those that were created.
    fn revert_snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        for path in snapshot.created.iter() {
            self.remove(path)?;
        }
        for (path, content) in snapshot.content.iter() {
            if !snapshot.created.contains(path) {
                self.write(path, content)?;
            }
        }
        Ok(())
    }

    /// Creates a snapshot from the provided paths, appends it to the snapshots list, and returns its identifier.
    fn snapshot(&mut self, paths: &[PathBuf]) -> Result<u64> {
        let snap = self.create_snapshot(paths)?;
        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        self.snapshots.push((id, snap));
        Ok(id)
    }

    /// Applies a patch by taking a snapshot of all files to be modified, then attempts to apply each change in the patch.
    /// If any change fails, the error is collected in a vector of (change, error) tuples.
    /// Returns a tuple containing the snapshot ID and a vector of failed changes.
    pub fn patch(&mut self, patch: &Patch) -> Result<PatchInfo> {
        let id = self.snapshot(&patch.affected_files())?;
        let mut pinfo = PatchInfo {
            rollback_id: id,
            succeeded: 0,
            should_continue: false,
            failures: Vec::new(),
        };
        for change in &patch.changes {
            match change {
                Change::Write(write_file) => {
                    if let Err(e) = self.write(write_file.path.as_path(), &write_file.content) {
                        pinfo.add_failure(change.clone(), e)?;
                    } else {
                        pinfo.succeeded += 1;
                    }
                }
                Change::Replace(replace) => {
                    let res = (|| {
                        let original = self.read(replace.path.as_path())?;
                        let new_content = replace.apply(&original)?;
                        self.write(replace.path.as_path(), &new_content)
                    })();
                    if let Err(e) = res {
                        pinfo.add_failure(change.clone(), e)?;
                    } else {
                        pinfo.succeeded += 1;
                    }
                }
                Change::View(_) => {
                    pinfo.should_continue = true;
                    pinfo.succeeded += 1;
                }
            }
        }
        Ok(pinfo)
    }

    /// Reverts all snapshots up to and including the given ID in reverse order, then removes them from the snapshots list.
    pub fn revert(&mut self, id: u64) -> Result<()> {
        let mut to_revert = Vec::new();
        let mut remaining = Vec::new();
        for pair in self.snapshots.drain(..) {
            if pair.0 <= id {
                to_revert.push(pair);
            } else {
                remaining.push(pair);
            }
        }
        if to_revert.is_empty() {
            return Err(Error::Internal(format!("Snapshot id {} not found", id)));
        }
        for (_id, snap) in to_revert.into_iter().rev() {
            self.revert_snapshot(snap)?;
        }
        self.snapshots = remaining;
        Ok(())
    }

    /// Lists all files from both the memory and directory stores.
    pub fn list(&self) -> Result<Vec<PathBuf>> {
        let mut files = self.memory.list()?;
        if let Some(ref fs) = self.directory {
            files.extend(fs.list()?);
        }
        Ok(files)
    }

    /// Returns the files that were last changed between the given snapshot ids, inclusive. Returns
    /// an empty list if no snapshots exist.
    pub fn last_changed_between(
        &self,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<Vec<PathBuf>> {
        if self.snapshots.is_empty() {
            return Ok(vec![]);
        }
        let min_id = start.unwrap_or_else(|| self.snapshots.first().unwrap().0);
        let max_id = end.unwrap_or_else(|| self.snapshots.last().unwrap().0);
        let mut latest: HashMap<PathBuf, u64> = HashMap::new();
        for (snap_id, snap) in &self.snapshots {
            for path in snap.touched() {
                latest
                    .entry(path)
                    .and_modify(|e| {
                        if *snap_id > *e {
                            *e = *snap_id
                        }
                    })
                    .or_insert(*snap_id);
            }
        }
        let mut result: Vec<PathBuf> = latest
            .into_iter()
            .filter_map(|(path, id)| {
                if id >= min_id && id <= max_id {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
        result.sort();
        Ok(result)
    }

    /// Returns a unique, sorted list of all files touched in the current session.
    /// This includes all files that have been modified in any snapshot.
    pub fn touched(&self) -> Result<Vec<PathBuf>> {
        if self.snapshots.is_empty() {
            return Ok(vec![]);
        }

        let mut files = std::collections::BTreeSet::new();

        for (_, snap) in &self.snapshots {
            for path in snap.touched() {
                files.insert(path);
            }
        }

        Ok(files.into_iter().collect())
    }

    /// Returns the original content of a file from the first snapshot where it appears.
    /// If the file was created in the first snapshot, returns an empty string.
    /// If the file does not occur in any snapshot, returns None.
    pub fn original(&self, path: &Path) -> Option<String> {
        if self.snapshots.is_empty() {
            return None;
        }

        // Sort snapshots by ID to find the first one
        let mut sorted_snapshots: Vec<&(u64, Snapshot)> = self.snapshots.iter().collect();
        sorted_snapshots.sort_by_key(|(id, _)| *id);

        for (_, snap) in sorted_snapshots {
            // First check if there's content for this file
            if let Some(content) = snap.content.get(path) {
                return Some(content.clone());
            }
            // If there's no content but file is in created list, it was a new empty file
            else if snap.created.contains(&path.to_path_buf()) {
                return Some(String::new());
            }
        }

        None
    }

    /// Matches files in both the memory and directory stores based on the provided patterns.
    /// The patterns are normalized using the substore's root (empty for memory) and the given current
    /// working directory, and matched using globset.
    pub fn find<T>(&self, cwd: T, patterns: Vec<String>) -> Result<Vec<PathBuf>>
    where
        T: abspath::IntoAbsPath,
    {
        let cwd = cwd.into_abs_path()?;
        let mut results = HashSet::new();

        // First, handle memory store with path cleaning
        let mem_files = self.memory.list()?;
        for pattern in &patterns {
            let cleaned = path_clean::clean(pattern);
            let pattern_str = cleaned.to_str().ok_or_else(|| {
                Error::Internal("Failed to convert cleaned path to string".to_string())
            })?;
            let glob = Glob::new(pattern_str).map_err(|e| Error::Path(e.to_string()))?;
            let matcher = glob.compile_matcher();
            for file in &mem_files {
                if matcher.is_match(file) {
                    results.insert(file.clone());
                }
            }
        }

        // Then handle directory store with path normalization for non-memory patterns
        if let Some(ref dir) = self.directory {
            let dir_files = dir.list()?;
            for pattern in &patterns {
                if pattern.starts_with(MEM_PREFIX) {
                    continue;
                }
                let normalized = files::normalize_path(dir.root.clone(), cwd.clone(), pattern)?;
                let pattern_str = normalized.to_str().ok_or_else(|| {
                    Error::Internal("Failed to convert normalized path to string".to_string())
                })?;
                let glob = Glob::new(pattern_str).map_err(|e| Error::Path(e.to_string()))?;
                let matcher = glob.compile_matcher();
                for file in &dir_files {
                    if matcher.is_match(file) {
                        results.insert(file.clone());
                    }
                }
            }
        }

        let mut result_vec: Vec<_> = results.into_iter().collect();
        result_vec.sort();
        Ok(result_vec)
    }

    /// Creates and dispatches a view patch for files matching the provided patterns. Expands the
    /// patterns using the current working directory, creates a Change::View for each matched path,
    /// and applies the patch. Returns a tuple of (snapshot ID, file count) from applying the patch.
    pub fn view<P>(&mut self, cwd: P, patterns: Vec<String>) -> Result<(u64, usize)>
    where
        P: abspath::IntoAbsPath,
    {
        let paths = self.find(cwd, patterns)?;
        let file_count = paths.len();
        let changes: Vec<Change> = paths.into_iter().map(patch::Change::View).collect();
        let patch = Patch { changes };
        let patch_info = self.patch(&patch)?;
        // Failures for view changes should always be empty.
        debug_assert!(patch_info.failures.is_empty());
        Ok((patch_info.rollback_id, file_count))
    }

    /// Add an empty patch to the snapshot sequence and return a snapshot ID. Useful as a markder.
    pub fn mark(&mut self) -> Result<u64> {
        let patch = Patch { changes: vec![] };
        let patch_info = self.patch(&patch)?;
        // Failures for view changes should always be empty.
        debug_assert!(patch_info.failures.is_empty());
        Ok(patch_info.rollback_id)
    }
}

#[cfg(test)]
mod tests {
    use super::abspath::AbsPath;
    use super::*;
    // test imports are used below
    use std::{collections::HashMap, fs, path::PathBuf};
    use tempfile::TempDir;

    #[test]
    fn test_state_with_filesystem() -> Result<()> {
        let temp_dir = TempDir::new().expect("failed to create temporary directory");
        let root = temp_dir.path().to_path_buf();

        // Create a test file in the temporary directory.
        let test_file = root.join("test.rs");
        fs::write(&test_file, "fn main() {}")?;

        // Create a Filesystem with a glob pattern for .rs files.
        let state = State::default()
            .with_directory(AbsPath::new(root.clone())?, vec!["*.rs".to_string()])?;

        // Get the filesystem from the state and list the files.
        let file_system = state.directory.as_ref().expect("Filesystem should be set");
        let files = file_system.list().unwrap();

        // Check that the test file is found (relative path).
        assert!(files.contains(&PathBuf::from("test.rs")));

        Ok(())
    }

    #[test]
    fn test_state_write() -> Result<()> {
        let temp_dir = TempDir::new().expect("failed to create temporary directory");
        // Setup filesystem
        let root = temp_dir.path().to_path_buf();
        let mut state = State::default()
            .with_directory(AbsPath::new(root.clone())?, vec!["*.txt".to_string()])?;

        // Test writing to filesystem
        state.write(Path::new("test.txt"), "file content")?;
        assert_eq!(state.read(Path::new("test.txt"))?, "file content");

        // Test writing to memory
        state.write(Path::new("::test.txt"), "memory content")?;
        assert_eq!(state.read(Path::new("::test.txt"))?, "memory content");

        Ok(())
    }

    #[test]
    fn test_state_read() -> Result<()> {
        struct TestCase {
            name: &'static str,
            fs_content: Option<&'static str>,
            memory_content: Option<&'static str>,
            path: &'static str,
            expected: Result<&'static str>,
        }

        let cases = vec![
            TestCase {
                name: "get from memory only",
                fs_content: None,
                memory_content: Some("memory content"),
                path: "::test.txt",
                expected: Ok("memory content"),
            },
            TestCase {
                name: "get from filesystem",
                fs_content: Some("file content"),
                memory_content: None,
                path: "test.txt",
                expected: Ok("file content"),
            },
            TestCase {
                name: "memory takes precedence over filesystem",
                fs_content: Some("file content"),
                memory_content: Some("memory content"),
                path: "test.txt",
                expected: Ok("memory content"),
            },
            TestCase {
                name: "no store configured",
                fs_content: None,
                memory_content: None,
                path: "test.txt",
                expected: Err(Error::NotFound {
                    msg: "No matching store".to_string(),
                    path: "test.txt".to_string(),
                }),
            },
            TestCase {
                name: "missing file in filesystem",
                fs_content: Some("file content"),
                memory_content: None,
                path: "nonexistent.txt",
                expected: Err(Error::NotFound {
                    msg: "File not found".to_string(),
                    path: "nonexistent.txt".to_string(),
                }),
            },
        ];

        for case in cases.into_iter() {
            // Setup temporary directory if we need filesystem
            let temp_dir = TempDir::new().expect("failed to create temporary directory");
            let mut state = State::default();

            // Setup filesystem if content provided
            if let Some(content) = case.fs_content {
                let root = temp_dir.path().to_path_buf();
                let test_file = root.join("test.txt");
                fs::write(&test_file, content)?;
                state = state.with_directory(AbsPath::new(root)?, vec!["*.txt".to_string()])?;
            }

            // Setup memory if content provided
            if let Some(content) = case.memory_content {
                let _ = state.dispatch_mut(Path::new(case.path), |store| {
                    store.write(Path::new(case.path), content)
                });
            }

            // Test the get operation
            let result = state.read(Path::new(case.path));

            match case.expected {
                Ok(expected) => {
                    assert!(
                        result.is_ok(),
                        "{}: expected Ok but got {:?}",
                        case.name,
                        result
                    );
                    assert_eq!(result.unwrap(), expected, "{}: content mismatch", case.name);
                }
                Err(_) => {
                    assert!(
                        result.is_err(),
                        "{}: expected Err but got Ok({:?})",
                        case.name,
                        result
                    );
                    let err = result.unwrap_err();
                    if let Error::NotFound { msg, path } = err {
                        assert_eq!(
                            Error::NotFound { msg, path },
                            match &case.expected {
                                Err(expected) => expected.clone(),
                                _ => panic!("Expected error variant"),
                            },
                            "{}: error message mismatch",
                            case.name
                        );
                    } else {
                        panic!("{}: unexpected error type: {:?}", case.name, err);
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_last_changed_between() -> Result<()> {
        struct TestCase {
            name: &'static str,
            patches: Vec<Patch>,
            start: Option<u64>,
            end: Option<u64>,
            expected: Result<Vec<&'static str>>,
        }

        let cases = vec![
            TestCase {
                name: "empty snapshots list",
                patches: vec![],
                start: None,
                end: None,
                expected: Ok(vec![]),
            },
            TestCase {
                name: "single snapshot",
                patches: vec![Patch {
                    changes: vec![
                        Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        }),
                        Change::Write(patch::WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        }),
                    ],
                }],
                start: Some(0),
                end: Some(0),
                expected: Ok(vec!["::a.txt", "::b.txt"]),
            },
            TestCase {
                name: "overlapping changes in range",
                patches: vec![
                    Patch {
                        changes: vec![
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::a.txt"),
                                content: "A0".to_string(),
                            }),
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::b.txt"),
                                content: "B0".to_string(),
                            }),
                        ],
                    },
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B1".to_string(),
                        })],
                    },
                ],
                start: Some(0),
                end: Some(0),
                expected: Ok(vec!["::a.txt"]),
            },
            TestCase {
                name: "full range with implicit boundaries",
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::c.txt"),
                            content: "C0".to_string(),
                        })],
                    },
                ],
                start: None,
                end: None,
                expected: Ok(vec!["::a.txt", "::b.txt", "::c.txt"]),
            },
            TestCase {
                name: "middle range",
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::c.txt"),
                            content: "C0".to_string(),
                        })],
                    },
                ],
                start: Some(1),
                end: Some(1),
                expected: Ok(vec!["::b.txt"]),
            },
            TestCase {
                name: "changes outside range excluded",
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(patch::WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A1".to_string(),
                        })],
                    },
                ],
                start: Some(1),
                end: Some(1),
                expected: Ok(vec!["::b.txt"]),
            },
            TestCase {
                name: "multiple files in multiple snapshots",
                patches: vec![
                    Patch {
                        changes: vec![
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::a.txt"),
                                content: "A0".to_string(),
                            }),
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::b.txt"),
                                content: "B0".to_string(),
                            }),
                        ],
                    },
                    Patch {
                        changes: vec![
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::c.txt"),
                                content: "C0".to_string(),
                            }),
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::d.txt"),
                                content: "D0".to_string(),
                            }),
                        ],
                    },
                    Patch {
                        changes: vec![
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::b.txt"),
                                content: "B1".to_string(),
                            }),
                            Change::Write(patch::WriteFile {
                                path: PathBuf::from("::d.txt"),
                                content: "D1".to_string(),
                            }),
                        ],
                    },
                ],
                start: Some(0),
                end: Some(1),
                expected: Ok(vec!["::a.txt", "::c.txt"]),
            },
        ];

        for case in cases {
            let mut state = State::default();

            // Apply each patch to build up the snapshot history
            for patch in case.patches {
                let patch_info = state.patch(&patch)?;
                assert!(
                    patch_info.failures.is_empty(),
                    "{}: patch application failed",
                    case.name
                );
            }

            // Test last_changed_between
            let result = state.last_changed_between(case.start, case.end);

            match (result, case.expected) {
                (Ok(got), Ok(expected)) => {
                    let got: Vec<&str> = got.iter().map(|p| p.to_str().unwrap()).collect();
                    assert_eq!(got, expected, "{}: got wrong paths", case.name);
                }
                (Err(Error::Internal(got)), Err(Error::Internal(expected))) => {
                    assert_eq!(got, expected, "{}: got wrong error message", case.name);
                }
                (got, expected) => {
                    panic!("{}: got {:?}, expected {:?}", case.name, got, expected);
                }
            }
        }

        Ok(())
    }

    /// Unit test for multiple snapshot layers.
    #[test]
    fn test_multiple_snapshot_layers() -> Result<()> {
        let mut state = State::default();

        let key_a = "::a.txt";
        let key_x = "::x.txt";
        state.dispatch_mut(Path::new(key_a), |store| {
            store.write(Path::new(key_a), "A0")
        })?;
        state.dispatch_mut(Path::new(key_x), |store| {
            store.write(Path::new(key_x), "X0")
        })?;

        let paths = vec![PathBuf::from(key_a), PathBuf::from(key_x)];
        let snap_id0 = state.snapshot(&paths)?;
        assert_eq!(snap_id0, 0);

        state.write(Path::new(key_a), "A1")?;
        state.write(Path::new(key_x), "X1")?;

        let snap_id1 = state.snapshot(&paths)?;
        assert_eq!(snap_id1, 1);

        state.write(Path::new(key_a), "A2")?;
        state.write(Path::new(key_x), "X2")?;

        assert_eq!(state.read(Path::new(key_a))?, "A2");
        assert_eq!(state.read(Path::new(key_x))?, "X2");

        state.revert(1)?;

        assert_eq!(state.read(Path::new(key_a))?, "A0");
        assert_eq!(state.read(Path::new(key_x))?, "X0");

        Ok(())
    }

    #[test]
    fn test_find() -> Result<()> {
        type TestSetup = Box<dyn Fn(&mut State) -> Result<Option<TempDir>>>;
        struct TestCase {
            name: &'static str,
            setup: TestSetup,
            patterns: Vec<&'static str>,
            expected: Vec<&'static str>,
        }

        let cases = vec![
            TestCase {
                name: "memory only - exact match",
                setup: Box::new(|state| {
                    state.write(Path::new("::foo.txt"), "foo")?;
                    state.write(Path::new("::bar.txt"), "bar")?;
                    Ok(None)
                }),
                patterns: vec!["::foo.txt"],
                expected: vec!["::foo.txt"],
            },
            TestCase {
                name: "memory only - dupes",
                setup: Box::new(|state| {
                    state.write(Path::new("::foo.txt"), "foo")?;
                    state.write(Path::new("::bar.txt"), "bar")?;
                    Ok(None)
                }),
                patterns: vec!["::foo.txt", "::foo.txt"],
                expected: vec!["::foo.txt"],
            },
            TestCase {
                name: "memory only - glob match",
                setup: Box::new(|state| {
                    state.write(Path::new("::foo.txt"), "foo")?;
                    state.write(Path::new("::bar.txt"), "bar")?;
                    Ok(None)
                }),
                patterns: vec!["::*.txt"],
                expected: vec!["::bar.txt", "::foo.txt"],
            },
            TestCase {
                name: "filesystem only",
                setup: Box::new(|state| {
                    let temp_dir = TempDir::new().expect("failed to create temporary directory");
                    let root = temp_dir.path().to_path_buf();
                    fs::write(root.join("foo.txt"), "foo")?;
                    fs::write(root.join("bar.txt"), "bar")?;
                    *state = state
                        .clone()
                        .with_directory(AbsPath::new(root)?, vec!["*.txt".to_string()])?;
                    Ok(Some(temp_dir))
                }),
                patterns: vec!["*.txt"],
                expected: vec!["bar.txt", "foo.txt"],
            },
            TestCase {
                name: "both stores - mixed patterns",
                setup: Box::new(|state| {
                    let temp_dir = TempDir::new().expect("failed to create temporary directory");
                    let root = temp_dir.path().to_path_buf();
                    fs::write(root.join("fs.txt"), "fs")?;
                    state.write(Path::new("::mem.txt"), "mem")?;
                    *state = state
                        .clone()
                        .with_directory(AbsPath::new(root)?, vec!["*.txt".to_string()])?;
                    Ok(Some(temp_dir))
                }),
                patterns: vec!["*.txt", "::*.txt"],
                expected: vec!["::mem.txt", "fs.txt"],
            },
            TestCase {
                name: "no matches",
                setup: Box::new(|state| {
                    state.write(Path::new("::foo.txt"), "foo")?;
                    Ok(None)
                }),
                patterns: vec!["::nonexistent.txt"],
                expected: vec![],
            },
            TestCase {
                name: "multiple patterns",
                setup: Box::new(|state| {
                    state.write(Path::new("::foo.txt"), "foo")?;
                    state.write(Path::new("::bar.rs"), "bar")?;
                    Ok(None)
                }),
                patterns: vec!["::*.txt", "::*.rs"],
                expected: vec!["::bar.rs", "::foo.txt"],
            },
        ];

        let cwd = AbsPath::new(std::path::PathBuf::from("/"))?;

        for case in cases {
            let mut guards: Vec<TempDir> = Vec::new();
            let mut state = State::default();
            if let Some(guard) = (case.setup)(&mut state)? {
                guards.push(guard);
            }

            let patterns: Vec<String> = case.patterns.iter().map(|s| s.to_string()).collect();
            let results = state.find(cwd.clone(), patterns)?;

            let result_strs: Vec<String> = results
                .iter()
                .filter_map(|p| p.to_str().map(String::from))
                .collect();
            let expected: Vec<String> = case.expected.into_iter().map(String::from).collect();

            assert_eq!(
                result_strs, expected,
                "{}: expected {:?}, got {:?}",
                case.name, expected, result_strs
            );
        }

        Ok(())
    }

    #[test]
    fn test_original() -> Result<()> {
        struct TestCase {
            name: &'static str,
            initial_files: HashMap<PathBuf, String>,
            patches: Vec<Patch>,
            path: &'static str,
            expected: Option<&'static str>,
        }

        let cases = vec![
            TestCase {
                name: "empty snapshots list",
                initial_files: HashMap::new(),
                patches: vec![],
                path: "::nonexistent.txt",
                expected: None,
            },
            TestCase {
                name: "newly created file in patch",
                initial_files: HashMap::new(),
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A1".to_string(),
                        })],
                    },
                ],
                path: "::a.txt",
                expected: Some(""),
            },
            TestCase {
                name: "file with initial content modified in patches",
                initial_files: {
                    let mut files = HashMap::new();
                    files.insert(PathBuf::from("::a.txt"), "Original".to_string());
                    files
                },
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A1".to_string(),
                        })],
                    },
                ],
                path: "::a.txt",
                expected: Some("Original"),
            },
            TestCase {
                name: "file in second snapshot only",
                initial_files: HashMap::new(),
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        })],
                    },
                ],
                path: "::b.txt",
                expected: Some(""),
            },
            TestCase {
                name: "file created in first snapshot",
                initial_files: HashMap::new(),
                patches: vec![Patch {
                    changes: vec![Change::View(PathBuf::from("::created.txt"))],
                }],
                path: "::created.txt",
                expected: Some(""),
            },
            TestCase {
                name: "file not in any snapshot",
                initial_files: {
                    let mut files = HashMap::new();
                    files.insert(PathBuf::from("::a.txt"), "A0".to_string());
                    files
                },
                patches: vec![Patch {
                    changes: vec![Change::Write(WriteFile {
                        path: PathBuf::from("::a.txt"),
                        content: "A1".to_string(),
                    })],
                }],
                path: "::nonexistent.txt",
                expected: None,
            },
        ];

        for case in cases {
            // Initialize state with pre-populated memory content
            let mut state = State::default().with_memory(case.initial_files)?;

            // Apply each patch to build up the snapshot history
            for patch in case.patches {
                let patch_info = state.patch(&patch)?;
                assert!(
                    patch_info.failures.is_empty(),
                    "{}: patch application failed",
                    case.name
                );
            }

            // Test original method
            let result = state.original(Path::new(case.path));

            match (result, case.expected) {
                (Some(got), Some(expected)) => {
                    assert_eq!(got, expected, "{}: got wrong content", case.name);
                }
                (None, None) => {
                    // Both are None, that's correct
                }
                (got, expected) => {
                    panic!("{}: got {:?}, expected {:?}", case.name, got, expected);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_touched() -> Result<()> {
        struct TestCase {
            name: &'static str,
            patches: Vec<Patch>,
            expected: Result<Vec<&'static str>>,
        }

        let cases = vec![
            TestCase {
                name: "empty snapshots list",
                patches: vec![],
                expected: Ok(vec![]),
            },
            TestCase {
                name: "single snapshot with multiple files",
                patches: vec![Patch {
                    changes: vec![
                        Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        }),
                        Change::Write(WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        }),
                    ],
                }],
                expected: Ok(vec!["::a.txt", "::b.txt"]),
            },
            TestCase {
                name: "multiple snapshots with unique files",
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::c.txt"),
                            content: "C0".to_string(),
                        })],
                    },
                ],
                expected: Ok(vec!["::a.txt", "::b.txt", "::c.txt"]),
            },
            TestCase {
                name: "multiple snapshots with overlapping files",
                patches: vec![
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::b.txt"),
                            content: "B0".to_string(),
                        })],
                    },
                    Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("::a.txt"),
                            content: "A1".to_string(),
                        })],
                    },
                ],
                expected: Ok(vec!["::a.txt", "::b.txt"]),
            },
            TestCase {
                name: "multiple snapshots with multiple files per snapshot",
                patches: vec![
                    Patch {
                        changes: vec![
                            Change::Write(WriteFile {
                                path: PathBuf::from("::a.txt"),
                                content: "A0".to_string(),
                            }),
                            Change::Write(WriteFile {
                                path: PathBuf::from("::b.txt"),
                                content: "B0".to_string(),
                            }),
                        ],
                    },
                    Patch {
                        changes: vec![
                            Change::Write(WriteFile {
                                path: PathBuf::from("::c.txt"),
                                content: "C0".to_string(),
                            }),
                            Change::Write(WriteFile {
                                path: PathBuf::from("::d.txt"),
                                content: "D0".to_string(),
                            }),
                        ],
                    },
                    Patch {
                        changes: vec![
                            Change::Write(WriteFile {
                                path: PathBuf::from("::b.txt"),
                                content: "B1".to_string(),
                            }),
                            Change::Write(WriteFile {
                                path: PathBuf::from("::d.txt"),
                                content: "D1".to_string(),
                            }),
                        ],
                    },
                ],
                expected: Ok(vec!["::a.txt", "::b.txt", "::c.txt", "::d.txt"]),
            },
            TestCase {
                name: "view changes included",
                patches: vec![
                    Patch {
                        changes: vec![Change::View(PathBuf::from("::view1.txt"))],
                    },
                    Patch {
                        changes: vec![
                            Change::View(PathBuf::from("::view2.txt")),
                            Change::Write(WriteFile {
                                path: PathBuf::from("::a.txt"),
                                content: "A0".to_string(),
                            }),
                        ],
                    },
                ],
                expected: Ok(vec!["::a.txt", "::view1.txt", "::view2.txt"]),
            },
        ];

        for case in cases {
            let mut state = State::default();

            // Apply each patch to build up the snapshot history
            for patch in case.patches {
                let patch_info = state.patch(&patch)?;
                assert!(
                    patch_info.failures.is_empty(),
                    "{}: patch application failed",
                    case.name
                );
            }

            // Test touched
            let result = state.touched();

            match (result, case.expected) {
                (Ok(got), Ok(expected)) => {
                    let got: Vec<&str> = got.iter().map(|p| p.to_str().unwrap()).collect();
                    assert_eq!(got, expected, "{}: got wrong paths", case.name);
                }
                (Err(Error::Internal(got)), Err(Error::Internal(expected))) => {
                    assert_eq!(got, expected, "{}: got wrong error message", case.name);
                }
                (got, expected) => {
                    panic!("{}: got {:?}, expected {:?}", case.name, got, expected);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_diff_path() -> Result<()> {
        struct TestCase {
            name: &'static str,
            orig_content: &'static str,
            current_content: &'static str,
            expected_type: PatchType,
            path: &'static str,
        }

        #[allow(dead_code)]
        enum PatchType {
            Write,
            Replace(usize), // number of replace operations
        }

        let cases = vec![
            TestCase {
                name: "small change - single line",
                orig_content: "Hello world",
                current_content: "Hello there",
                expected_type: PatchType::Write, // Single line changes also use Write
                path: "::test.txt",
            },
            TestCase {
                name: "small changes - multiple lines",
                orig_content: "Line 1\nLine 2\nLine 3\nLine 4\n",
                current_content: "Line 1\nLine 2 modified\nLine 3\nLine 4 modified\n",
                expected_type: PatchType::Write, // Use Write when 50% of lines change
                path: "::test.txt",
            },
            TestCase {
                name: "majority changed - use write",
                orig_content: "Line 1\nLine 2\nLine 3\nLine 4\n",
                current_content: "Line 1 changed\nLine 2 changed\nLine 3 changed\nLine 4\n",
                expected_type: PatchType::Write,
                path: "::test.txt",
            },
            TestCase {
                name: "completely different content",
                orig_content: "Original content",
                current_content: "Totally different content",
                expected_type: PatchType::Write,
                path: "::test.txt",
            },
            TestCase {
                name: "empty to non-empty",
                orig_content: "",
                current_content: "New content added",
                expected_type: PatchType::Write, // Empty to non-empty is a complete change
                path: "::test.txt",
            },
            TestCase {
                name: "non-empty to empty",
                orig_content: "Content to be removed",
                current_content: "",
                expected_type: PatchType::Write, // Complete removal is a 100% change
                path: "::test.txt",
            },
        ];

        for case in cases {
            let mut state = State::default();
            let path = PathBuf::from(case.path);

            // Setup initial state with original content
            let mut initial_files = HashMap::new();
            initial_files.insert(path.clone(), case.orig_content.to_string());
            state = state.with_memory(initial_files)?;

            // Take a snapshot of the initial state
            state.snapshot(&[path.clone()])?;

            // Update to current content
            state.write(&path, case.current_content)?;

            // Generate diff
            let patch = state.diff_path(path.clone())?;

            // Verify patch structure based on expected type
            match case.expected_type {
                PatchType::Write => {
                    assert_eq!(
                        patch.changes.len(),
                        1,
                        "{}: expected 1 change (Write), got {}",
                        case.name,
                        patch.changes.len()
                    );

                    match &patch.changes[0] {
                        Change::Write(w) => {
                            assert_eq!(
                                w.content, case.current_content,
                                "{}: Write content doesn't match expected",
                                case.name
                            );
                        }
                        _ => panic!(
                            "{}: expected Write change, got {:?}",
                            case.name, patch.changes[0]
                        ),
                    }
                }
                PatchType::Replace(count) => {
                    assert_eq!(
                        patch.changes.len(),
                        count,
                        "{}: expected {} Replace changes, got {}",
                        case.name,
                        count,
                        patch.changes.len()
                    );

                    for change in &patch.changes {
                        match change {
                            Change::Replace(_) => {} // This is expected
                            _ => panic!("{}: expected Replace change, got {:?}", case.name, change),
                        }
                    }
                }
            }

            // Verify the patch can transform original to current
            let mut new_state = State::default();
            let mut initial_files = HashMap::new();
            initial_files.insert(path.clone(), case.orig_content.to_string());
            new_state = new_state.with_memory(initial_files)?;

            let patch_info = new_state.patch(&patch)?;
            assert!(
                patch_info.failures.is_empty(),
                "{}: failed to apply patch: {:?}",
                case.name,
                patch_info.failures
            );

            let result = new_state.read(&path)?;
            assert_eq!(
                result, case.current_content,
                "{}: patched content doesn't match expected",
                case.name
            );
        }

        Ok(())
    }
}
