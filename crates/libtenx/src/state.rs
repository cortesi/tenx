use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::dialect::Dialects;
use crate::model::Models;

/// The serializable state of Tenx, which persists between invocations.
#[derive(Debug, Deserialize, Serialize)]
pub struct State {
    pub snapshot: HashMap<PathBuf, String>,
    pub working_directory: PathBuf,
    pub dialect: Dialects,
    pub model: Models,
}

impl State {
    /// Creates a new Context with the specified working directory and dialect.
    pub fn new<P: AsRef<Path>>(working_directory: P, dialect: Dialects, model: Models) -> Self {
        Self {
            snapshot: HashMap::new(),
            working_directory: working_directory.as_ref().to_path_buf(),
            model,
            dialect,
        }
    }
}

/// Manages the storage and retrieval of State objects.
pub struct StateStore {
    base_dir: PathBuf,
}

impl StateStore {
    /// Creates a new StateStore with the specified base directory.
    pub fn new<P: AsRef<Path>>(base_dir: Option<P>) -> Self {
        let base_dir = base_dir
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .expect("Failed to get config directory")
                    .join("tenx")
                    .join("state")
            });
        Self { base_dir }
    }

    /// Saves the given State to a file.
    pub fn save(&self, state: &State) -> std::io::Result<()> {
        let file_name = normalize_path(&state.working_directory);
        let file_path = self.base_dir.join(file_name);
        fs::create_dir_all(self.base_dir.as_path())?;
        let serialized = serde_json::to_string(state)?;
        fs::write(file_path, serialized)
    }

    /// Loads a State from a file based on the given working directory.
    pub fn load<P: AsRef<Path>>(&self, working_directory: P) -> std::io::Result<State> {
        let file_name = normalize_path(working_directory.as_ref());
        let file_path = self.base_dir.join(file_name);
        let serialized = fs::read_to_string(file_path)?;
        serde_json::from_str(&serialized)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

/// Normalizes a path for use as a filename by replacing problematic characters.
pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(['/', '\\'], "_")
        .replace([':', '<', '>', '"', '|', '?', '*'], "")
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
    fn test_state_store() {
        let temp_dir = TempDir::new().unwrap();
        let state_store = StateStore::new(Some(temp_dir.path()));

        let state = State::new(
            "/test/dir",
            Dialects::Tags(dialect::Tags {}),
            model::Models::Claude(model::Claude::default()),
        );
        state_store.save(&state).unwrap();

        let loaded_state = state_store.load("/test/dir").unwrap();
        assert_eq!(loaded_state.working_directory, state.working_directory);
        assert_eq!(loaded_state.dialect, state.dialect);
    }
}
