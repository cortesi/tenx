pub mod abspath;
pub mod files;

use std::{
    collections::HashMap,
    fs,
    path::{absolute, Path, PathBuf},
};

use crate::{
    error::{Result, TenxError},
    state::abspath::AbsPath,
};

pub const MEM_PREFIX: &str = "::";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
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
    pub fn abspath(&self, path: &Path) -> crate::Result<PathBuf> {
        let p = PathBuf::from(&*self.root).join(path);
        absolute(p.clone())
            .map_err(|e| TenxError::Internal(format!("could not absolute {}: {}", p.display(), e)))
    }

    /// Gets the content of a file by converting the input path to an absolute path and reading it.
    pub fn read(&self, path: &Path) -> crate::Result<String> {
        let abs_path = self.abspath(path)?;
        fs::read_to_string(&abs_path).map_err(|e| {
            TenxError::Internal(format!("Could not read file {}: {}", abs_path.display(), e))
        })
    }

    /// Writes content to a file, creating it if it doesn't exist or overwriting if it does.
    pub fn write(&self, path: &Path, content: &str) -> crate::Result<()> {
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
}

/// The state underlying a session. Presents a unified interface over an optional filesystem
/// directory and a memory store. In-memory file names are prefixed with "::"
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    directory: Option<Directory>,
    memory: HashMap<String, String>,
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
    pub fn create_memory(&mut self, key: String, value: String) {
        self.memory.insert(key, value);
    }

    /// Retrieves the content associated with the given path.
    /// If the path exists in memory, return that value. Otherwise, read from the file system.
    pub fn read(&self, path: &Path) -> crate::Result<String> {
        let key = path.to_string_lossy().to_string();
        if let Some(value) = self.memory.get(&key) {
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
    pub fn write(&mut self, path: &Path, content: &str) -> crate::Result<()> {
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

    /// Creates a snapshot of the given list of paths. For each path, if the file exists, its content is captured;
    /// otherwise, the path is marked as created.
    pub fn create_snapshot(&self, paths: &[PathBuf]) -> crate::Result<Snapshot> {
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

    /// Removes a file or memory entry for the given path.
    pub fn remove(&mut self, path: &Path) -> crate::Result<()> {
        let key = path.to_string_lossy().to_string();
        if key.starts_with(MEM_PREFIX) {
            self.memory.remove(&key);
            return Ok(());
        }
        if let Some(fs) = &self.directory {
            let abs_path = fs.abspath(path)?;
            if abs_path.exists() {
                std::fs::remove_file(&abs_path).map_err(|e| {
                    TenxError::Internal(format!(
                        "Could not remove file {}: {}",
                        abs_path.display(),
                        e
                    ))
                })?;
            }
            Ok(())
        } else {
            Err(TenxError::NotFound {
                msg: "No file system available".to_string(),
                path: key,
            })
        }
    }

    /// Reverts the state to the given snapshot.
    /// Restores content for existing files and memory entries, and removes files or memory entries that were created.
    pub fn revert(&mut self, snapshot: Snapshot) -> crate::Result<()> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    use tempfile::TempDir;

    #[test]
    fn test_state_with_filesystem() -> crate::Result<()> {
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
    fn test_state_write() -> crate::Result<()> {
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
    fn test_state_read() -> crate::Result<()> {
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
                state.create_memory(case.path.to_string(), content.to_string());
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
    fn test_create_and_revert_snapshot() -> crate::Result<()> {
        // Define an action for modifying state.
        enum Action {
            WriteFile {
                path: &'static str,
                content: &'static str,
            },
            WriteMemory {
                key: &'static str,
                content: &'static str,
            },
        }

        struct TestCase {
            name: &'static str,
            initial_files: Vec<(&'static str, &'static str)>,
            initial_memory: Vec<(&'static str, &'static str)>,
            snapshot_paths: Vec<&'static str>,
            modifications: Vec<Action>,
            expected: Vec<(&'static str, Option<&'static str>)>, // None means key should not exist
        }

        let test_cases = vec![
            TestCase {
                name: "Revert modifications and removals",
                initial_files: vec![("file1.txt", "original file content")],
                initial_memory: vec![("::mem1.txt", "original memory")],
                snapshot_paths: vec!["file1.txt", "::mem1.txt", "file2.txt"],
                modifications: vec![
                    Action::WriteFile {
                        path: "file1.txt",
                        content: "modified file content",
                    },
                    Action::WriteFile {
                        path: "file2.txt",
                        content: "new file content",
                    },
                    Action::WriteMemory {
                        key: "::mem1.txt",
                        content: "modified memory",
                    },
                    Action::WriteMemory {
                        key: "::mem2.txt",
                        content: "extra memory",
                    },
                ],
                expected: vec![
                    ("file1.txt", Some("original file content")),
                    ("file2.txt", None),
                    ("::mem1.txt", Some("original memory")),
                    ("::mem2.txt", Some("extra memory")),
                ],
            },
            TestCase {
                name: "No changes revert",
                initial_files: vec![("fileA.txt", "contentA")],
                initial_memory: vec![("::memA.txt", "contentA")],
                snapshot_paths: vec!["fileA.txt", "::memA.txt"],
                modifications: vec![],
                expected: vec![
                    ("fileA.txt", Some("contentA")),
                    ("::memA.txt", Some("contentA")),
                ],
            },
        ];

        for tc in test_cases {
            // Create a temp directory and initialize state.
            let temp_dir = TempDir::new().expect("failed to create temporary directory");
            let root = temp_dir.path().to_path_buf();
            let mut state = State::default();
            state.set_directory(AbsPath::new(root.clone())?, vec!["*.txt".to_string()])?;

            // Setup initial file system state.
            for (file, content) in tc.initial_files.iter() {
                state.write(Path::new(file), content)?;
            }
            // Setup initial memory state.
            for (key, content) in tc.initial_memory.iter() {
                state.create_memory(key.to_string(), content.to_string());
            }

            // Create snapshot over specified paths.
            let paths: Vec<PathBuf> = tc.snapshot_paths.iter().map(PathBuf::from).collect();
            let snapshot = state.create_snapshot(&paths)?;

            // Apply modifications.
            for action in tc.modifications.iter() {
                match action {
                    Action::WriteFile { path, content } => {
                        state.write(Path::new(path), content)?;
                    }
                    Action::WriteMemory { key, content } => {
                        state.write(Path::new(key), content)?;
                    }
                }
            }

            // Revert to snapshot.
            state.revert(snapshot)?;

            // Verify expected outcomes.
            for (path_str, expected_opt) in tc.expected.iter() {
                let path = Path::new(path_str);
                let result = state.read(path);
                match expected_opt {
                    Some(expected_content) => {
                        let actual = result.unwrap_or_else(|_| {
                            panic!("{}: expected content for {}", tc.name, path_str)
                        });
                        assert_eq!(
                            actual, *expected_content,
                            "{}: content mismatch for {}",
                            tc.name, path_str
                        );
                    }
                    None => {
                        assert!(
                            result.is_err(),
                            "{}: expected error for {}",
                            tc.name,
                            path_str
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
