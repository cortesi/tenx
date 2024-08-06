use crate::error::{ClaudeError, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct CrateConfig {
    pub manifest_path: PathBuf,
}

impl CrateConfig {
    pub fn discover<P: AsRef<Path>>(start_dir: P) -> Result<Self> {
        let manifest_path = Self::find_cargo_toml(start_dir.as_ref())?;
        Ok(CrateConfig { manifest_path })
    }

    fn find_cargo_toml(start_dir: &Path) -> Result<PathBuf> {
        for entry in WalkDir::new(start_dir).follow_links(true).into_iter() {
            let entry = entry.map_err(|e| ClaudeError::Io(e.into()))?;
            if entry.file_name() == "Cargo.toml" {
                return Ok(entry.path().to_path_buf());
            }
        }
        Err(ClaudeError::CargoTomlNotFound)
    }
}

#[derive(Debug)]
pub struct Query {
    pub working_directory: PathBuf,
    pub attach_paths: Vec<String>,
    pub edit_paths: Vec<String>,
    pub user_prompt: String,
    pub crate_config: Option<CrateConfig>,
}

impl Query {
    pub fn from_edits<P: AsRef<Path>>(edit_paths: Vec<P>) -> Result<Self> {
        if edit_paths.is_empty() {
            return Err(ClaudeError::NoEditPaths);
        }

        let first_path = edit_paths[0].as_ref();
        let working_directory = first_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let crate_config = CrateConfig::discover(&working_directory)?;

        Ok(Query {
            working_directory,
            attach_paths: Vec::new(),
            edit_paths: edit_paths
                .into_iter()
                .map(|p| p.as_ref().to_string_lossy().into_owned())
                .collect(),
            user_prompt: String::new(),
            crate_config: Some(crate_config),
        })
    }

    pub fn with_attach_path(mut self, path: String) -> Self {
        self.attach_paths.push(path);
        self
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.user_prompt = prompt.to_string();
        self
    }
}
