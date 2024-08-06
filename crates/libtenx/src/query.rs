use crate::error::{ClaudeError, Result};
use std::path::{Path, PathBuf};

use crate::crateops::Workspace;

#[derive(Debug)]
pub struct Query {
    pub attach_paths: Vec<PathBuf>,
    pub edit_paths: Vec<PathBuf>,
    pub user_prompt: String,
    pub crate_config: Option<Workspace>,
}

impl Query {
    pub fn from_edits<P: AsRef<Path>>(edit_paths: Vec<P>) -> Result<Self> {
        if edit_paths.is_empty() {
            return Err(ClaudeError::NoPathsProvided);
        }

        let edit_paths: Vec<PathBuf> = edit_paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();

        let crate_config = Workspace::discover(&edit_paths)?;

        Ok(Query {
            attach_paths: Vec::new(),
            edit_paths,
            user_prompt: String::new(),
            crate_config: Some(crate_config),
        })
    }

    pub fn with_attach_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.attach_paths.push(path.as_ref().to_path_buf());
        self
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.user_prompt = prompt.to_string();
        self
    }
}
