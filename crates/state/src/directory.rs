use std::{
    fs,
    path::{absolute, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    abspath::AbsPath,
    error::{Error, Result},
};

use super::files;
use super::SubStore;

/// A file system directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directory {
    pub root: AbsPath,
    globs: Vec<String>,
}

impl Directory {
    pub fn new(root: AbsPath, globs: Vec<String>) -> Result<Self> {
        Ok(Self { root, globs })
    }

    /// Converts a path relative to the root directory to an absolute path
    fn abspath(&self, path: &Path) -> Result<PathBuf> {
        // First normalize the path to ensure it doesn't escape the root
        let normalized = files::normalize_path(
            self.root.clone(),
            self.root.clone(),
            path.to_str()
                .ok_or_else(|| Error::Path("Invalid path encoding".to_string()))?,
        )?;

        let p = PathBuf::from(&*self.root).join(normalized);
        absolute(p.clone())
            .map_err(|e| Error::Internal(format!("could not absolute {}: {}", p.display(), e)))
    }

    /// List files in the directory using ignore rules, returning all included files relative to
    /// project root.
    ///
    /// Applies the `FileSystem` glob patterns and respects .gitignore and other ignore files. Glob
    /// patterns can be positive (include) or negative (exclude, prefixed with !).
    ///
    /// Files are sorted by path.
    pub fn list(&self) -> Result<Vec<PathBuf>> {
        files::list_files(self.root.clone(), self.globs.clone())
    }

    /// Gets the content of a file by converting the input path to an absolute path and reading it.
    pub fn read(&self, path: &Path) -> Result<String> {
        let abs_path = self.abspath(path)?;
        if !abs_path.exists() {
            return Err(Error::NotFound {
                msg: "File not found".to_string(),
                path: path.display().to_string(),
            });
        }
        fs::read_to_string(&abs_path).map_err(|e| {
            Error::Internal(format!("Could not read file {}: {}", abs_path.display(), e))
        })
    }

    /// Writes content to a file, creating it if it doesn't exist or overwriting if it does.
    pub fn write(&mut self, path: &Path, content: &str) -> Result<()> {
        let abs_path = self.abspath(path)?;
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                Error::Internal(format!(
                    "Could not create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        fs::write(&abs_path, content)
            .map_err(|e| Error::Internal(format!("Could not write file {}: {}", path.display(), e)))
    }

    /// Removes a file by joining the given path with the root directory.
    pub fn remove(&mut self, path: &Path) -> Result<()> {
        let abs_path = self.root.join(path);
        if !abs_path.exists() {
            return Err(Error::NotFound {
                msg: "File not found".to_string(),
                path: path.display().to_string(),
            });
        }
        fs::remove_file(&abs_path).map_err(|e| {
            Error::Internal(format!(
                "Could not remove file {}: {}",
                abs_path.display(),
                e
            ))
        })
    }
}

impl SubStore for Directory {
    fn list(&self) -> Result<Vec<PathBuf>> {
        self.list()
    }

    fn read(&self, path: &Path) -> Result<String> {
        self.read(path)
    }

    fn write(&mut self, path: &Path, content: &str) -> Result<()> {
        self.write(path, content)
    }

    fn remove(&mut self, path: &Path) -> Result<()> {
        self.remove(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_directory_path_security() -> Result<()> {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let root = AbsPath::new(temp_dir.path().to_path_buf())?;
        let mut dir = Directory::new(root.clone(), vec![])?;

        // Setup: create a test file
        dir.write(Path::new("test.txt"), "test content")?;
        assert_eq!(dir.read(Path::new("test.txt"))?, "test content");

        // Test path traversal attempts
        for attempt in &[
            "../secret.txt",
            "../../etc/passwd",
            "subdir/../../secret.txt",
        ] {
            assert!(
                dir.read(Path::new(attempt)).is_err(),
                "Read {} should fail",
                attempt
            );
            assert!(
                dir.write(Path::new(attempt), "bad").is_err(),
                "Write {} should fail",
                attempt
            );
            assert!(
                dir.remove(Path::new(attempt)).is_err(),
                "Remove {} should fail",
                attempt
            );
        }

        // Test absolute paths - should be confined to root
        for path in &["/etc/passwd", "/tmp/test.txt"] {
            if dir.write(Path::new(path), "test").is_ok() {
                let abs_path = dir.abspath(Path::new(path))?;
                assert!(abs_path.starts_with(&*root), "{} escaped root", path);
                dir.remove(Path::new(path)).ok();
            }
        }

        // Verify valid paths work
        dir.write(Path::new("subdir/nested.txt"), "nested")?;
        assert_eq!(dir.read(Path::new("subdir/nested.txt"))?, "nested");

        // Verify list only returns files under root
        for file in dir.list()? {
            assert!(
                root.join(&file).starts_with(&*root),
                "{:?} outside root",
                file
            );
        }

        Ok(())
    }

    #[test]
    fn test_directory_symlink_security() -> Result<()> {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let root = AbsPath::new(temp_dir.path().to_path_buf())?;
        let mut dir = Directory::new(root.clone(), vec![])?;

        // Create outside target
        let outside_dir = TempDir::new().expect("failed to create outside dir");
        let outside_file = outside_dir.path().join("secret.txt");
        std::fs::write(&outside_file, "secret")?;

        // Create symlinks pointing outside root
        #[cfg(unix)]
        use std::os::unix::fs::symlink;
        #[cfg(windows)]
        use std::os::windows::fs::symlink_file;

        let link_path = temp_dir.path().join("link");
        #[cfg(unix)]
        symlink(&outside_file, &link_path).unwrap();
        #[cfg(windows)]
        symlink_file(&outside_file, &link_path).unwrap();

        // Test symlink access (documents known limitation)
        if dir.read(Path::new("link")).is_ok() {
            println!("Warning: Symlinks can escape root directory");
        }
        if dir.write(Path::new("link"), "modified").is_ok() {
            if std::fs::read_to_string(&outside_file).unwrap() == "modified" {
                println!("Warning: Can write through symlinks");
            }
        }

        Ok(())
    }
}
