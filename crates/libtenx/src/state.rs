use std::{
    collections::HashMap,
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
