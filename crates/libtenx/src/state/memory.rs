use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::SubStore;
use crate::error::{Result, TenxError};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Memory {
    memory: HashMap<PathBuf, String>,
}

impl SubStore for Memory {
    fn list(&self) -> Result<Vec<PathBuf>> {
        let mut paths: Vec<PathBuf> = self.memory.keys().cloned().collect();
        paths.sort();
        Ok(paths)
    }

    fn read(&self, path: &std::path::Path) -> Result<String> {
        self.memory
            .get(path)
            .cloned()
            .ok_or_else(|| TenxError::NotFound {
                msg: "Memory entry not found".to_string(),
                path: path.display().to_string(),
            })
    }

    fn write(&mut self, path: &std::path::Path, content: &str) -> Result<()> {
        self.memory.insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn remove(&mut self, path: &std::path::Path) -> Result<()> {
        self.memory.remove(path);
        Ok(())
    }
}
