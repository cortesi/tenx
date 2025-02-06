use super::SubStore;
use crate::Result;
use std::{collections::HashMap, path::PathBuf};

#[derive(Default)]
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
            .ok_or_else(|| crate::TenxError::NotFound {
                msg: "Memory entry not found".to_string(),
                path: path.display().to_string(),
            })
    }

    fn write(&self, path: &std::path::Path, content: &str) -> Result<()> {
        let mut this = self.memory.clone();
        this.insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn remove(&self, path: &std::path::Path) -> Result<()> {
        let mut this = self.memory.clone();
        this.remove(path);
        Ok(())
    }
}
