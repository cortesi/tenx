pub mod abspath;
pub mod files;

use std::{
    collections::HashMap,
    path::{absolute, Path, PathBuf},
};

use crate::error::{Result, TenxError};

pub const MEM_PREFIX: &str = "::";

use crate::state::abspath::AbsPath;

/// A file system.
pub struct Directory {
    root: AbsPath,
    globs: Vec<String>,
}

impl Directory {
    pub fn new(root: PathBuf, globs: Vec<String>) -> Result<Self> {
        Ok(Self {
            root: AbsPath::new(root)?,
            globs,
        })
    }

    /// List files in the directory using ignore rules, returning all included files relative to
    /// project root.
    ///
    /// Applies the `FileSystem` glob patterns and respects .gitignore and other ignore files. Glob
    /// patterns can be positive (include) or negative (exclude, prefixed with !).
    ///
    /// Files are sorted by path.
    pub fn list_files(&self) -> Result<Vec<PathBuf>> {
        files::walk_files(self.root.clone(), self.globs.clone())
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
    directory: Option<Directory>,
    memory: HashMap<String, String>,
}

impl State {
    /// Set the directory path and glob patterns for file operations.
    pub fn set_directory(&mut self, root: PathBuf, globs: Vec<String>) -> Result<()> {
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
        let mut state = State::default();
        state.set_directory(root.clone(), vec!["*.rs".to_string()])?;

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
        state.set_directory(root.clone(), vec!["*.txt".to_string()])?;

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
                state.set_directory(root, vec!["*.txt".to_string()])?;
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
}
