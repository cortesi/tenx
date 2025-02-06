use std::{
    fs,
    path::{absolute, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, TenxError},
    state::abspath::AbsPath,
};

use super::files;
use super::SubStore;

/// A file system directory
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    root: AbsPath,
    globs: Vec<String>,
}

impl Directory {
    pub fn new(root: AbsPath, globs: Vec<String>) -> Result<Self> {
        Ok(Self { root, globs })
    }

    /// Converts a path relative to the root directory to an absolute path
    fn abspath(&self, path: &Path) -> Result<PathBuf> {
        let p = PathBuf::from(&*self.root).join(path);
        absolute(p.clone())
            .map_err(|e| TenxError::Internal(format!("could not absolute {}: {}", p.display(), e)))
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

impl SubStore for Directory {
    fn list(&self) -> Result<Vec<PathBuf>> {
        self.list()
    }

    fn read(&self, path: &Path) -> Result<String> {
        self.read(path)
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        self.write(path, content)
    }

    fn remove(&self, path: &Path) -> Result<()> {
        self.remove(path)
    }
}
