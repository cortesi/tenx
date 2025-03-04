//! A PathBuf wrapper that guarantees the enclosed path is absolute.
use std::{
    fmt,
    ops::Deref,
    path::{Path, PathBuf},
};

use crate::error::{Result, TenxError};
use serde::{Deserialize, Serialize};

/// A helper trait to convert a type into an AbsPath.
pub trait IntoAbsPath {
    fn into_abs_path(self) -> Result<AbsPath>;
}

impl IntoAbsPath for AbsPath {
    fn into_abs_path(self) -> Result<AbsPath> {
        Ok(self)
    }
}

impl IntoAbsPath for PathBuf {
    fn into_abs_path(self) -> Result<AbsPath> {
        AbsPath::new(self)
    }
}

impl IntoAbsPath for &PathBuf {
    fn into_abs_path(self) -> Result<AbsPath> {
        AbsPath::new(self.to_path_buf())
    }
}

/// A PathBuf wrapper that guarantees the enclosed path is absolute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AbsPath(PathBuf);

impl AbsPath {
    /// Creates a new AbsPath, ensuring the path is absolute.
    pub fn new(path: PathBuf) -> Result<Self> {
        if !path.is_absolute() {
            return Err(TenxError::Internal(format!(
                "Path must be absolute: {}",
                path.display()
            )));
        }
        Ok(AbsPath(path))
    }
}

impl TryFrom<&PathBuf> for AbsPath {
    type Error = TenxError;

    fn try_from(path: &PathBuf) -> std::result::Result<Self, Self::Error> {
        Self::new(path.to_path_buf())
    }
}

impl TryFrom<PathBuf> for AbsPath {
    type Error = TenxError;

    fn try_from(path: PathBuf) -> std::result::Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl AsRef<Path> for AbsPath {
    fn as_ref(&self) -> &Path {
        self.0.as_path()
    }
}

impl Deref for AbsPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.0.as_path()
    }
}

impl fmt::Display for AbsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}
