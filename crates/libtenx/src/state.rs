use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::dialect::Dialects;
use serde::{Deserialize, Serialize};

/// The serializable state of Tenx.
#[derive(Debug, Deserialize, Serialize)]
pub struct State {
    pub snapshot: HashMap<PathBuf, String>,
    pub working_directory: PathBuf,
    pub dialect: Dialects,
}

impl State {
    /// Creates a new Context with the specified working directory and dialect.
    pub fn new<P: AsRef<Path>>(working_directory: P, dialect: Dialects) -> Self {
        Self {
            snapshot: HashMap::new(),
            working_directory: working_directory.as_ref().to_path_buf(),
            dialect,
        }
    }
}
