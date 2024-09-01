use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteFile {
    pub path: PathBuf,
    pub content: String,
}

impl WriteFile {
    /// Applies the write operation to the given file content in the cache.
    pub fn apply_to_cache(&self, cache: &mut HashMap<PathBuf, String>) -> Result<()> {
        cache.insert(self.path.clone(), self.content.clone());
        Ok(())
    }
}
