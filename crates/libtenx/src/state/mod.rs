pub mod files;

use std::{
    collections::HashMap,
    path::{absolute, Path, PathBuf},
};

use crate::error::TenxError;

pub const MEM_PREFIX: &str = "::";

/// A file system.
pub struct FileSystem {
    root: PathBuf,
    globs: Vec<String>,
}

impl FileSystem {
    pub fn new(root: PathBuf, globs: Vec<String>) -> Self {
        Self { root, globs }
    }

    pub fn walk(&self) -> Vec<PathBuf> {
        files::walk_files(self.root.clone(), self.globs.clone()).unwrap()
    }

    /// Converts a path relative to the root directory to an absolute path
    pub fn abspath(&self, path: &Path) -> crate::Result<PathBuf> {
        let p = self.root.join(path);
        absolute(p.clone())
            .map_err(|e| TenxError::Internal(format!("could not absolute {}: {}", p.display(), e)))
    }

    /// Gets the content of a file by converting the input path to an absolute path and reading it.
    pub fn read(&self, path: &Path) -> crate::Result<String> {
        let abs_path = self.abspath(path)?;
        std::fs::read_to_string(&abs_path).map_err(|e| {
            TenxError::Internal(format!("Could not read file {}: {}", abs_path.display(), e))
        })
    }

    /// Writes content to a file, creating it if it doesn't exist or overwriting if it does.
    pub fn write(&self, path: &Path, content: &str) -> crate::Result<()> {
        let abs_path = self.abspath(path)?;
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                TenxError::Internal(format!(
                    "Could not create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        std::fs::write(&abs_path, content).map_err(|e| {
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
#[derive(Default)]
pub struct State {
    file_system: Option<FileSystem>,
    memory: HashMap<String, String>,
}

impl State {
    /// Set the file system to the given value.
    pub fn set_file_system(&mut self, file_system: FileSystem) {
        self.file_system = Some(file_system);
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

        match &self.file_system {
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

        match &self.file_system {
            Some(fs) => fs.write(path, content),
            None => Err(TenxError::NotFound {
                msg: "No file system available".to_string(),
                path: path.display().to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_state_with_filesystem() -> crate::Result<()> {
        let temp_dir = TempDir::new().expect("failed to create temporary directory");
        let root = temp_dir.path().to_path_buf();

        // Create a test file in the temporary directory.
        let test_file = root.join("test.rs");
        fs::write(&test_file, "fn main() {}")?;

        // Create a Filesystem with a glob pattern for .rs files.
        let fsystem = FileSystem::new(root.clone(), vec!["*.rs".to_string()]);

        // Create a State and set its Filesystem.
        let mut state = State::default();
        state.set_file_system(fsystem);

        // Get the filesystem from the state and list the files.
        let file_system = state
            .file_system
            .as_ref()
            .expect("Filesystem should be set");
        let files = file_system.walk();

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
        state.set_file_system(FileSystem::new(root.clone(), vec!["*.txt".to_string()]));

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
            expected: Result<&'static str, &'static str>,
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
                expected: Err("No file system available: test.txt"),
            },
            TestCase {
                name: "missing file in filesystem",
                fs_content: Some("file content"),
                memory_content: None,
                path: "nonexistent.txt",
                expected: Err("File not found: nonexistent.txt"),
            },
        ];

        for case in cases {
            // Setup temporary directory if we need filesystem
            let temp_dir = TempDir::new().expect("failed to create temporary directory");
            let mut state = State::default();

            // Setup filesystem if content provided
            if let Some(content) = case.fs_content {
                let root = temp_dir.path().to_path_buf();
                let test_file = root.join("test.txt");
                fs::write(&test_file, content)?;
                state.set_file_system(FileSystem::new(root, vec!["*.txt".to_string()]));
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
                Err(expected_err) => {
                    assert!(
                        result.is_err(),
                        "{}: expected Err but got Ok({:?})",
                        case.name,
                        result
                    );
                    let err = result.unwrap_err();
                    if let TenxError::NotFound { msg, path } = err {
                        let error_string = format!("{}: {}", msg, path);
                        assert_eq!(
                            error_string, expected_err,
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
}
