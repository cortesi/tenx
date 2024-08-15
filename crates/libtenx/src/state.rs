use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use colored::*;
use libruskel::Ruskel;
use serde::{Deserialize, Serialize};

use crate::{
    dialect::Dialect,
    model::{Model, ModelProvider},
    prompt::PromptInput,
    Result, TenxError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextType {
    Ruskel,
    File,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ContextData {
    /// Unresolved content that should be read from a file
    Path(PathBuf),
    /// Unresolved content that will be resolved in accord with DocType.
    Unresolved(String),
    /// Resolved content that can be passed to the model.
    Resolved(String),
}

/// Reference material included in the prompt.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Context {
    /// The type of documentation.
    pub ty: ContextType,
    /// The name of the documentation.
    pub name: String,
    /// The contents of the help document.
    pub contents: ContextData,
}

impl Context {
    /// Resolves the contents of the documentation.
    pub fn resolve(&mut self) -> Result<()> {
        self.contents =
            match std::mem::replace(&mut self.contents, ContextData::Resolved(String::new())) {
                ContextData::Path(path) => {
                    ContextData::Resolved(std::fs::read_to_string(path).map_err(TenxError::Io)?)
                }
                ContextData::Unresolved(content) => match self.ty {
                    ContextType::Ruskel => {
                        let ruskel = Ruskel::new(&content);
                        ContextData::Resolved(
                            ruskel
                                .render(false, false)
                                .map_err(|e| TenxError::Resolve(e.to_string()))?,
                        )
                    }
                    ContextType::File => {
                        return Err(TenxError::Resolve(
                            "Cannot resolve unresolved Text content".to_string(),
                        ))
                    }
                },
                resolved @ ContextData::Resolved(_) => resolved,
            };
        Ok(())
    }

    /// Converts a Docs to a string representation.
    pub fn to_string(&self) -> Result<String> {
        match &self.contents {
            ContextData::Resolved(content) => Ok(content.clone()),
            _ => Err(TenxError::Parse("Unresolved doc content".to_string())),
        }
    }
}

/// The serializable state of Tenx, which persists between invocations.
#[derive(Debug, Deserialize, Serialize)]
pub struct Session {
    pub snapshot: HashMap<PathBuf, String>,
    pub working_directory: PathBuf,
    pub dialect: Dialect,
    pub model: Option<Model>,
    pub prompt_inputs: Vec<PromptInput>,
    pub context: Vec<Context>,
}

impl Session {
    /// Creates a new Context with the specified working directory and dialect.
    pub fn new<P: AsRef<Path>>(working_directory: P, dialect: Dialect, model: Model) -> Self {
        Self {
            snapshot: HashMap::new(),
            working_directory: working_directory.as_ref().to_path_buf(),
            model: Some(model),
            dialect,
            prompt_inputs: vec![],
            context: vec![],
        }
    }

    /// Pretty prints the State information.
    pub fn pretty_print(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "{}\n{:?}\n\n",
            "Working Directory:".blue().bold(),
            self.working_directory
        ));

        output.push_str(&format!("{}\n", "Files in Snapshot:".blue().bold()));
        for path in self.snapshot.keys() {
            output.push_str(&format!("  - {:?}\n", path));
        }
        output.push('\n');

        output.push_str(&format!(
            "{}\n{:?}\n\n",
            "Dialect:".blue().bold(),
            self.dialect
        ));

        output.push_str(&format!("{}\n", "Model:".blue().bold()));
        output.push_str(
            &self
                .model
                .as_ref()
                .map_or(String::new(), |m| m.pretty_print()),
        );

        output
    }
}

/// Manages the storage and retrieval of State objects.
pub struct StateStore {
    base_dir: PathBuf,
}

impl StateStore {
    /// Creates a new StateStore with the specified base directory.
    /// Creates a new StateStore with the specified base directory.
    pub fn new<P: AsRef<Path>>(base_dir: Option<P>) -> std::io::Result<Self> {
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
        let file_name = normalize_path(&state.working_directory);
        let file_path = self.base_dir.join(file_name);
        let serialized = serde_json::to_string(state)?;
        fs::write(file_path, serialized)
    }

    /// Loads a State from a file based on the given working directory.
    pub fn load<P: AsRef<Path>>(&self, working_directory: P) -> std::io::Result<Session> {
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
    fn test_state_store() -> std::io::Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let state_store = StateStore::new(Some(temp_dir.path()))?;

        let state = Session::new(
            "/test/dir",
            Dialect::Tags(dialect::Tags {}),
            model::Model::Claude(model::Claude::default()),
        );
        state_store.save(&state)?;

        let loaded_state = state_store.load("/test/dir")?;
        assert_eq!(loaded_state.working_directory, state.working_directory);
        assert_eq!(loaded_state.dialect, state.dialect);
        Ok(())
    }
}
