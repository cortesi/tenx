use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{Result, Session, TenxError};

/// Normalizes a path for use as a filename by replacing problematic characters.
pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(['/', '\\'], "_")
        .replace([':', '<', '>', '"', '|', '?', '*'], "")
}

/// Manages the storage and retrieval of State objects.
pub struct SessionStore {
    base_dir: PathBuf,
}

impl SessionStore {
    /// Creates a new StateStore with the specified base directory.
    /// Creates a new StateStore with the specified base directory.
    pub fn open<P: AsRef<Path>>(base_dir: Option<P>) -> std::io::Result<Self> {
        let base_dir = base_dir
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .expect("Failed to get home directory")
                    .join(".config")
                    .join("tenx")
                    .join("state")
            });
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Saves the given State to a file.
    pub fn save(&self, state: &Session) -> std::io::Result<()> {
        let file_name = normalize_path(&state.root);
        let file_path = self.base_dir.join(file_name);
        let serialized = serde_json::to_string(state)?;
        fs::write(file_path, serialized)
    }

    /// Loads a State from a file based on the given working directory.
    pub fn load<P: AsRef<Path>>(&self, working_directory: P) -> Result<Session> {
        let file_name = normalize_path(working_directory.as_ref());
        let file_path = self.base_dir.join(file_name);
        let serialized = fs::read_to_string(file_path)?;
        serde_json::from_str(&serialized).map_err(|e| TenxError::Internal(format!("{}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialect, model};
    use tempfile::TempDir;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path(Path::new("/foo/bar")), "_foo_bar");
        assert_eq!(
            normalize_path(Path::new("C:\\Windows\\System32")),
            "C_Windows_System32"
        );
        assert_eq!(normalize_path(Path::new("file:name.txt")), "filename.txt");
    }

    #[test]
    fn test_state_store() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let state_store = SessionStore::open(Some(temp_dir.path()))?;

        let state = Session::new(
            Some(temp_dir.path().to_path_buf()),
            dialect::Dialect::Tags(dialect::Tags {}),
            model::Model::Claude(model::Claude::default()),
        );
        state_store.save(&state)?;

        let loaded_state = state_store.load(temp_dir.path().canonicalize().unwrap())?;
        assert_eq!(loaded_state.root, state.root);
        assert_eq!(loaded_state.dialect, state.dialect);
        Ok(())
    }
}
