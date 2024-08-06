use crate::error::{ClaudeError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Workspace {
    pub manifest_path: PathBuf,
}

impl Workspace {
    pub fn discover<P: AsRef<Path>>(paths: &[P]) -> Result<Self> {
        let common_ancestor = Self::find_common_ancestor(paths)?;
        let manifest_path = Self::find_enclosing_cargo_toml(&common_ancestor)?;
        Ok(Workspace { manifest_path })
    }

    fn find_common_ancestor<P: AsRef<Path>>(paths: &[P]) -> Result<PathBuf> {
        if paths.is_empty() {
            return Err(ClaudeError::NoPathsProvided);
        }

        let mut common_ancestor = paths[0].as_ref().to_path_buf();
        for path in paths.iter().skip(1) {
            while !path.as_ref().starts_with(&common_ancestor) {
                if !common_ancestor.pop() {
                    return Err(ClaudeError::NoCommonAncestor);
                }
            }
        }

        Ok(common_ancestor)
    }

    fn find_enclosing_cargo_toml(start_dir: &Path) -> Result<PathBuf> {
        let mut current_dir = start_dir.to_path_buf();
        loop {
            let cargo_toml = current_dir.join("Cargo.toml");
            if cargo_toml.exists() {
                return Ok(cargo_toml);
            }
            if !current_dir.pop() {
                break;
            }
        }
        Err(ClaudeError::CargoTomlNotFound)
    }
}
