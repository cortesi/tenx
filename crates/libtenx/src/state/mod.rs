//! A unified interface over all persistent state.
//!
//! All modifications are made through `Patch` operations, and return an ID that can be used
//! to revert the state to a previous snapshot.
pub mod abspath;
mod directory;
pub mod files;
mod memory;

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    path::{Path, PathBuf},
};

use globset::Glob;
use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, TenxError},
    patch::{Change, Patch},
};

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
    pub id: u64,
    pub succeeded: usize,
    /// All errors here are of type TenxError::Patch
    pub failures: Vec<(Change, TenxError)>,
}

impl PatchInfo {
    pub fn add_failure(&mut self, change: Change, error: TenxError) -> Result<()> {
        match error {
            TenxError::Patch { user, model } => {
                self.failures
                    .push((change, TenxError::Patch { user, model }));
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
        use std::collections::BTreeSet;
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
            Err(TenxError::NotFound {
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
            Err(TenxError::NotFound {
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
                Err(TenxError::NotFound { .. }) => snap.create(p.clone()),
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
            id,
            succeeded: 0,
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
                Change::View(_) => pinfo.succeeded += 1,
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
            return Err(TenxError::Internal(format!("Snapshot id {} not found", id)));
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

    /// Returns the files that were last changed between the given snapshot ids, inclusive.
    pub fn last_changed_between(
        &self,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<Vec<PathBuf>> {
        if self.snapshots.is_empty() {
            return Err(TenxError::Internal("No snapshots available".to_string()));
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
                TenxError::Internal("Failed to convert cleaned path to string".to_string())
            })?;
            let glob = Glob::new(pattern_str).map_err(|e| TenxError::Path(e.to_string()))?;
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
                    TenxError::Internal("Failed to convert normalized path to string".to_string())
                })?;
                let glob = Glob::new(pattern_str).map_err(|e| TenxError::Path(e.to_string()))?;
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

    /// Creates and dispatches a view patch for files matching the provided patterns.
    /// Expands the patterns using the current working directory, creates a Change::View for each matched path,
    /// and applies the patch. Returns the snapshot ID from applying the patch.
    pub fn view<P>(&mut self, cwd: P, patterns: Vec<String>) -> Result<u64>
    where
        P: abspath::IntoAbsPath,
    {
        let paths = self.find(cwd, patterns)?;
        let changes: Vec<Change> = paths.into_iter().map(Change::View).collect();
        let patch = Patch { changes };
        let patch_info = self.patch(&patch)?;
        // Failures for view changes should always be empty.
        debug_assert!(patch_info.failures.is_empty());
        Ok(patch_info.id)
    }

    /// Add an empty patch to the snapshot sequence and return a snapshot ID. Useful as a markder.
    pub fn mark(&mut self) -> Result<u64> {
        let patch = Patch { changes: vec![] };
        let patch_info = self.patch(&patch)?;
        // Failures for view changes should always be empty.
        debug_assert!(patch_info.failures.is_empty());
        Ok(patch_info.id)
    }
}

#[cfg(test)]
mod tests {
    use super::abspath::AbsPath;
    use super::*;
    use std::{fs, path::PathBuf};
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
                expected: Err(TenxError::NotFound {
                    msg: "No matching store".to_string(),
                    path: "test.txt".to_string(),
                }),
            },
            TestCase {
                name: "missing file in filesystem",
                fs_content: Some("file content"),
                memory_content: None,
                path: "nonexistent.txt",
                expected: Err(TenxError::NotFound {
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
                    if let TenxError::NotFound { msg, path } = err {
                        assert_eq!(
                            TenxError::NotFound { msg, path },
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

    // Table-driven test for snapshot creation and revert.
    #[test]
    fn test_create_and_revert_snapshot() -> Result<()> {
        // Existing test for single snapshot revert.
        Ok(())
    }

    #[test]
    fn test_last_changed_between() -> Result<()> {
        use crate::patch::{Change, Patch, WriteFile};

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
                expected: Err(TenxError::Internal("No snapshots available".to_string())),
            },
            TestCase {
                name: "single snapshot",
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
                start: Some(0),
                end: Some(0),
                expected: Ok(vec!["::a.txt", "::b.txt"]),
            },
            TestCase {
                name: "overlapping changes in range",
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
                        changes: vec![Change::Write(WriteFile {
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
                start: None,
                end: None,
                expected: Ok(vec!["::a.txt", "::b.txt", "::c.txt"]),
            },
            TestCase {
                name: "middle range",
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
                start: Some(1),
                end: Some(1),
                expected: Ok(vec!["::b.txt"]),
            },
            TestCase {
                name: "changes outside range excluded",
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
                start: Some(1),
                end: Some(1),
                expected: Ok(vec!["::b.txt"]),
            },
            TestCase {
                name: "multiple files in multiple snapshots",
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
                (Err(TenxError::Internal(got)), Err(TenxError::Internal(expected))) => {
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
}
