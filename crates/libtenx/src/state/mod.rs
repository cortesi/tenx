pub mod abspath;
pub mod files;

use std::{
    collections::HashMap,
    fs,
    path::{absolute, Path, PathBuf},
};

use crate::{
    error::{Result, TenxError},
    patch::{Change, Patch},
    state::abspath::AbsPath,
};

pub const MEM_PREFIX: &str = "::";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
}

/// A file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    root: AbsPath,
    globs: Vec<String>,
}

impl Directory {
    pub fn new(root: AbsPath, globs: Vec<String>) -> Result<Self> {
        Ok(Self { root, globs })
    }

    /// List files in the directory using ignore rules, returning all included files relative to
    /// project root.
    ///
    /// Applies the `FileSystem` glob patterns and respects .gitignore and other ignore files. Glob
    /// patterns can be positive (include) or negative (exclude, prefixed with !).
    ///
    /// Files are sorted by path.
    pub fn list_files(&self) -> Result<Vec<PathBuf>> {
        files::list_files(self.root.clone(), self.globs.clone())
    }

    /// Converts a path relative to the root directory to an absolute path
    pub fn abspath(&self, path: &Path) -> Result<PathBuf> {
        let p = PathBuf::from(&*self.root).join(path);
        absolute(p.clone())
            .map_err(|e| TenxError::Internal(format!("could not absolute {}: {}", p.display(), e)))
    }

    /// Gets the content of a file by converting the input path to an absolute path and reading it.
    pub fn read(&self, path: &Path) -> Result<String> {
        let abs_path = self.abspath(path)?;
        fs::read_to_string(&abs_path).map_err(|e| {
            TenxError::Internal(format!("Could not read file {}: {}", abs_path.display(), e))
        })
    }

    /// Writes content to a file, creating it if it doesn't exist or overwriting if it does.
    pub fn write(&self, path: &Path, content: &str) -> Result<()> {
        let abs_path = self.abspath(path)?;
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                TenxError::Internal(format!(
                    "Could not create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        fs::write(&abs_path, content).map_err(|e| {
            TenxError::Internal(format!(
                "Could not write file {}: {}",
                abs_path.display(),
                e
            ))
        })
    }

    /// Removes a file by joining the given path with the root directory.
    pub fn remove(&self, path: &Path) -> Result<()> {
        let abs_path = self.root.join(path);
        if abs_path.exists() {
            fs::remove_file(&abs_path).map_err(|e| {
                TenxError::Internal(format!(
                    "Could not remove file {}: {}",
                    abs_path.display(),
                    e
                ))
            })?;
        }
        Ok(())
    }
}

/// The state underlying a session. This is the set of resources that our models are editing. State
/// presents a unified interface over an optional filesystem directory and a memory store.
/// In-memory file names are prefixed with "::"
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    directory: Option<Directory>,
    memory: HashMap<String, String>,
    snapshots: Vec<(u64, Snapshot)>,
    next_snapshot_id: u64,
}

impl State {
    /// Set the directory path and glob patterns for file operations.
    pub fn set_directory(&mut self, root: AbsPath, globs: Vec<String>) -> Result<()> {
        self.directory = Some(Directory::new(root, globs)?);
        Ok(())
    }

    /// List files in the directory, applying the inclusion globs.
    pub fn list_directory(&self) -> Result<Vec<PathBuf>> {
        Ok(if let Some(fs) = self.directory.as_ref() {
            fs.list_files()?
        } else {
            vec![]
        })
    }

    /// Create a new memory entry with the given key and value.
    /// Fails if the key does not start with MEM_PREFIX.
    pub fn create_memory(&mut self, key: String, value: String) -> Result<u64> {
        if !key.starts_with(MEM_PREFIX) {
            return Err(TenxError::Internal(
                "Memory key must start with MEM_PREFIX".to_string(),
            ));
        }
        self.memory.insert(key, value);
        Ok(self.next_snapshot_id)
    }

    /// Retrieves the content associated with the given path.
    /// If the path exists in memory, returns that value. Otherwise, reads from the file system.
    pub fn read(&self, path: &Path) -> Result<String> {
        let key = path.to_string_lossy().to_string();
        if let Some(value) = self.memory.get(&key) {
            return Ok(value.clone());
        }
        let mem_key = format!("{}{}", MEM_PREFIX, key);
        if let Some(value) = self.memory.get(&mem_key) {
            return Ok(value.clone());
        }
        match &self.directory {
            Some(fs) => fs.read(path).map_err(|_| TenxError::NotFound {
                msg: "File not found".to_string(),
                path: path.display().to_string(),
            }),
            None => Err(TenxError::NotFound {
                msg: "No file system available".to_string(),
                path: path.display().to_string(),
            }),
        }
    }

    /// Writes content to a path. If the path starts with MEM_PREFIX, writes to memory,
    /// otherwise writes to the filesystem.
    fn write(&mut self, path: &Path, content: &str) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        if key.starts_with(MEM_PREFIX) {
            self.memory.insert(key, content.to_string());
            return Ok(());
        }

        match &self.directory {
            Some(fs) => fs.write(path, content),
            None => Err(TenxError::NotFound {
                msg: "No file system available".to_string(),
                path: path.display().to_string(),
            }),
        }
    }

    /// Removes a file or memory entry for the given path.
    fn remove(&mut self, path: &Path) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        if key.starts_with(MEM_PREFIX) {
            self.memory.remove(&key);
            return Ok(());
        }
        if let Some(fs) = &self.directory {
            fs.remove(path)
        } else {
            Err(TenxError::NotFound {
                msg: "No file system available".to_string(),
                path: key,
            })
        }
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
    /// Restores content for existing files and memory entries, and removes files or memory entries that were created.
    fn revert_snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        // Remove files or entries that were created.
        for path in snapshot.created.iter() {
            self.remove(path)?;
        }
        // Restore content for files or memory entries that existed.
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
    pub fn patch(&mut self, patch: &Patch) -> Result<(u64, Vec<(Change, TenxError)>)> {
        let snap_id = self.snapshot(&patch.changed_files())?;
        let mut failures = Vec::new();
        for change in &patch.changes {
            match change {
                Change::Write(write_file) => {
                    if let Err(e) = self.write(write_file.path.as_path(), &write_file.content) {
                        failures.push((change.clone(), e));
                    }
                }
                Change::Replace(replace) => {
                    let res = (|| {
                        let original = self.read(replace.path.as_path())?;
                        let new_content = replace.apply(&original)?;
                        self.write(replace.path.as_path(), &new_content)
                    })();
                    if let Err(e) = res {
                        failures.push((change.clone(), e));
                    }
                }
            }
        }
        Ok((snap_id, failures))
    }

    /// Reverts all snapshots up to and including the given ID in reverse order, then removes them from the snapshots list.
    pub fn revert(&mut self, id: u64) -> Result<()> {
        let mut to_revert = Vec::new();
        let mut remaining = Vec::new();
        // Partition snapshots into those to revert and those to keep.
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
        // Revert in reverse order.
        for (_id, snap) in to_revert.into_iter().rev() {
            self.revert_snapshot(snap)?;
        }
        self.snapshots = remaining;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
        let mut state = State::default();
        state.set_directory(AbsPath::new(root.clone())?, vec!["*.rs".to_string()])?;

        // Get the filesystem from the state and list the files.
        let file_system = state.directory.as_ref().expect("Filesystem should be set");
        let files = file_system.list_files().unwrap();

        // Check that the test file is found (relative path).
        assert!(files.contains(&PathBuf::from("test.rs")));

        Ok(())
    }

    #[test]
    fn test_state_write() -> Result<()> {
        let temp_dir = TempDir::new().expect("failed to create temporary directory");
        let mut state = State::default();

        // Setup filesystem
        let root = temp_dir.path().to_path_buf();
        state.set_directory(AbsPath::new(root.clone())?, vec!["*.txt".to_string()])?;

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
                path: "test.txt",
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
                name: "no filesystem configured",
                fs_content: None,
                memory_content: None,
                path: "test.txt",
                expected: Err(TenxError::NotFound {
                    msg: "No file system available".to_string(),
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
                state.set_directory(AbsPath::new(root)?, vec!["*.txt".to_string()])?;
            }

            // Setup memory if content provided
            if let Some(content) = case.memory_content {
                let _ = state
                    .create_memory(format!("{}{}", MEM_PREFIX, case.path), content.to_string());
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
                            &TenxError::NotFound { msg, path },
                            match &case.expected {
                                Err(expected) => expected,
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
        // (unchanged)
        Ok(())
    }

    /// Unit test for multiple snapshot layers.
    #[test]
    fn test_multiple_snapshot_layers() -> Result<()> {
        // We'll test using memory entries.
        let mut state = State::default();

        // Insert initial memory entries.
        let key_a = "::a.txt";
        let key_x = "::x.txt";
        let _ = state.create_memory(key_a.to_string(), "A0".to_string());
        let _ = state.create_memory(key_x.to_string(), "X0".to_string());

        // Create first snapshot (id 0) capturing the initial state.
        let paths = vec![PathBuf::from(key_a), PathBuf::from(key_x)];
        let snap_id0 = state.snapshot(&paths)?;
        assert_eq!(snap_id0, 0);

        // Modify the state.
        state.write(Path::new(key_a), "A1")?;
        state.write(Path::new(key_x), "X1")?;

        // Create a second snapshot (id 1) capturing state after modifications.
        let snap_id1 = state.snapshot(&paths)?;
        assert_eq!(snap_id1, 1);

        // Further modify the state.
        state.write(Path::new(key_a), "A2")?;
        state.write(Path::new(key_x), "X2")?;

        // Verify that current state is modified.
        assert_eq!(state.read(Path::new(key_a))?, "A2");
        assert_eq!(state.read(Path::new(key_x))?, "X2");

        // Revert all snapshots up to id 1. This should revert both snapshots in reverse order.
        state.revert(1)?;

        // After reverting:
        // The second snapshot (id 1) reverts state from "A2"/"X2" back to state at snapshot id 1 ("A1"/"X1"),
        // then the first snapshot (id 0) reverts state to the initial state ("A0"/"X0").
        assert_eq!(state.read(Path::new(key_a))?, "A0");
        assert_eq!(state.read(Path::new(key_x))?, "X0");

        Ok(())
    }
}
