//! Session persistence module, handling storage and retrieval of sessions.

use crate::{
    config::Config,
    error::{Result, TenxError},
    session::Session,
};
use fs_err as fs;
use std::path::{Path, PathBuf};

/// Normalizes a path for use as a filename by replacing problematic characters.
pub fn path_to_filename(path: &Path) -> String {
    path.to_string_lossy()
        .replace(['/', '\\'], "_")
        .replace([':', '<', '>', '"', '|', '?', '*'], "")
}

/// Loads a session from a file located at a specific path.
pub fn load_session<P: AsRef<Path>>(path: P) -> Result<Session> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(TenxError::SessionStore(format!(
            "No such session: {}",
            path.display()
        )));
    }
    let serialized = fs::read_to_string(path)
        .map_err(|e| TenxError::SessionStore(format!("Failed to read session: {}", e)))?;
    serde_json::from_str(&serialized)
        .map_err(|e| TenxError::SessionStore(format!("Failed to parse session: {}", e)))
}

/// Manages persistent storage and retrieval of Session objects.
///
/// Sessions are stored in a directory structure, with each session serialized to JSON.
/// The store provides methods to save, load, and list available sessions.
pub struct SessionStore {
    base_dir: PathBuf,
}

impl SessionStore {
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

    /// Lists all sessions in the store.
    pub fn list(&self) -> Result<Vec<String>> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.base_dir)
            .map_err(|e| TenxError::SessionStore(format!("Failed to read directory: {}", e)))?
        {
            let entry = entry
                .map_err(|e| TenxError::SessionStore(format!("Failed to read entry: {}", e)))?;
            if entry
                .file_type()
                .map_err(|e| TenxError::SessionStore(format!("Failed to get file type: {}", e)))?
                .is_file()
            {
                if let Some(name) = entry.file_name().to_str() {
                    sessions.push(name.to_string());
                }
            }
        }
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Project;
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
        let config = Config {
            project: Project {
                root: temp_dir.path().into(),
                ..Default::default()
            },
            ..Default::default()
        };

        let state_store = SessionStore::open(temp_dir.path().into()).unwrap();

        let state = Session::new(&config)?;
        state_store.save_current(&config, &state).unwrap();
        state_store.save("test_session", &state).unwrap();

        let name = path_to_filename(&config.project_root());
        let _ = state_store.load(&name)?;

        let sessions = state_store.list()?;
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&name));
        assert!(sessions.contains(&"test_session".to_string()));

        Ok(())
    }
}
