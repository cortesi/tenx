use std::{
    fmt,
    ops::Deref,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// A wrapper around PathBuf that guarantees the enclosed path is absolute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AbsPath(PathBuf);

impl AbsPath {
    /// Creates a new AbsPath, ensuring the path is absolute.
    pub fn new(path: PathBuf) -> crate::Result<Self> {
        if !path.is_absolute() {
            return Err(crate::TenxError::Internal(format!(
                "Path must be absolute: {}",
                path.display()
            )));
        }
        Ok(AbsPath(path))
    }
}

impl TryFrom<PathBuf> for AbsPath {
    type Error = crate::TenxError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
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
