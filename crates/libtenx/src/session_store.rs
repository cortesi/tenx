use std::path::{Path, PathBuf};

use fs_err as fs;

use crate::{config::Config, Result, Session, TenxError};

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
    pub fn open(base_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Saves the given State to a file.
    pub fn save(&self, config: &Config, state: &Session) -> Result<()> {
        let file_name = normalize_path(&config.project_root());
        let file_path = self.base_dir.join(file_name);
        let serialized = serde_json::to_string(state)
            .map_err(|e| TenxError::SessionStore(format!("serialization failed: {}", e)))?;
        fs::write(&file_path, serialized)?;
        Ok(())
    }

    /// Loads a State from a file based on the given name.
    pub fn load<S: AsRef<str>>(&self, name: S) -> Result<Session> {
        let file_path = self.base_dir.join(name.as_ref());
        let serialized = fs::read_to_string(&file_path)?;
        serde_json::from_str(&serialized).map_err(|e| TenxError::Internal(format!("{}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectRoot;
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
        let mut config = Config::default();
        config.project_root = ProjectRoot::Path(temp_dir.path().into());

        let state_store = SessionStore::open(temp_dir.path().into()).unwrap();

        let state = Session::default();
        state_store.save(&config, &state).unwrap();

        let name = normalize_path(&config.project_root());
        let _ = state_store.load(name)?;
        Ok(())
    }
}
