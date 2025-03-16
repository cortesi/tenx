use std::path::PathBuf;

use crate::{config::Project, error::Result};

/// Walk project directory using ignore rules, returning all included files relative to project
/// root.
///
/// Applies project glob patterns and uses the ignore crate's functionality for respecting
/// .gitignore and other ignore files. Glob patterns can be positive (include) or negative
/// (exclude, prefixed with !).
use state::{abspath::AbsPath, files::list_files};

pub fn walk_project(project: &Project) -> Result<Vec<PathBuf>> {
    let root = AbsPath::new(project.root.clone())?;
    let ret = list_files(root, project.include.clone())?;
    Ok(ret)
}
