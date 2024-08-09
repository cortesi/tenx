use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// The serializable state of Tenx.
#[derive(Debug, Deserialize, Serialize)]
pub struct State {
    pub snapshot: HashMap<PathBuf, String>,
    pub working_directory: PathBuf,
}

impl State {
    /// Creates a new Context with the specified working directory.
    pub fn new<P: AsRef<Path>>(working_directory: P) -> Self {
        Self {
            snapshot: HashMap::new(),
            working_directory: working_directory.as_ref().to_path_buf(),
        }
    }
}

