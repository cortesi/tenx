use std::path::{Path, PathBuf};

use fs_err as fs;

use crate::{config::Config, Result, Session, TenxError};

/// Normalizes a path for use as a filename by replacing problematic characters.
pub fn path_to_filename(path: &Path) -> String {
    path.to_string_lossy()
        .replace(['/', '\\'], "_")
        .replace([':', '<', '>', '"', '|', '?', '*'], "")
}

/// Loads a session from a file located at a specific path.
pub fn load_session<P: AsRef<Path>>(path: P) -> Result<Session> {
    let serialized = fs::read_to_string(path)?;
    serde_json::from_str(&serialized).map_err(|e| TenxError::Internal(format!("{}", e)))
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

    /// Saves a session to the store with the specified name.
    pub fn save(&self, name: &str, state: &Session) -> Result<()> {
        let file_path = self.base_dir.join(name);
        let serialized = serde_json::to_string(state)
            .map_err(|e| TenxError::SessionStore(format!("serialization failed: {}", e)))?;
        fs::write(&file_path, serialized)?;
        Ok(())
    }

    /// Saves the given State to a the store, using the current directory identifier.
    pub fn save_current(&self, config: &Config, state: &Session) -> Result<()> {
        let file_name = path_to_filename(&config.project_root());
        self.save(&file_name, state)
    }

    /// Loads a State from a file based on the given name.
    pub fn load<S: AsRef<str>>(&self, name: S) -> Result<Session> {
        let file_path = self.base_dir.join(name.as_ref());
        load_session(file_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectRoot;
    use tempfile::TempDir;

    #[test]
    fn test_normalize_path() {
        assert_eq!(path_to_filename(Path::new("/foo/bar")), "_foo_bar");
        assert_eq!(
            path_to_filename(Path::new("C:\\Windows\\System32")),
            "C_Windows_System32"
        );
        assert_eq!(path_to_filename(Path::new("file:name.txt")), "filename.txt");
    }

    #[test]
    fn test_state_store() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.project_root = ProjectRoot::Path(temp_dir.path().into());

        let state_store = SessionStore::open(temp_dir.path().into()).unwrap();

        let state = Session::default();
        state_store.save_current(&config, &state).unwrap();

        let name = path_to_filename(&config.project_root());
        let _ = state_store.load(name)?;
        Ok(())
    }
}
