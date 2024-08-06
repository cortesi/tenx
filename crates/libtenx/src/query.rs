use crate::error::Result;
use std::path::{Path, PathBuf};

use crate::workspace::Workspace;

#[derive(Debug)]
pub struct Query {
    pub attach_paths: Vec<PathBuf>,
    pub edit_paths: Vec<PathBuf>,
    pub user_prompt: String,
    pub workspace: Workspace,
}

impl Query {
    pub fn from_edits<P: AsRef<Path>>(edit_paths: Vec<P>) -> Result<Self> {
        let edit_paths: Vec<PathBuf> = edit_paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();

        let workspace = Workspace::discover(&edit_paths)?;

        // Convert edit_paths to relative paths
        let relative_edit_paths = edit_paths
            .into_iter()
            .map(|p| workspace.relative_path(p))
            .collect::<Result<Vec<PathBuf>>>()?;

        Ok(Query {
            edit_paths: relative_edit_paths,
            workspace,
            attach_paths: Vec::new(),
            user_prompt: String::new(),
        })
    }

    pub fn with_attach_path<P: AsRef<Path>>(mut self, path: P) -> Result<Self> {
        let relative_path = self.workspace.relative_path(path)?;
        self.attach_paths.push(relative_path);
        Ok(self)
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.user_prompt = prompt.to_string();
        self
    }
}
