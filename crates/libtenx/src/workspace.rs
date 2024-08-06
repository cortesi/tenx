use std::path::{Path, PathBuf};

use crate::error::{ClaudeError, Result};

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
            return Err(ClaudeError::Workspace("No paths provided".to_string()));
        }

        let mut common_ancestor = paths[0].as_ref().to_path_buf();
        for path in paths.iter().skip(1) {
            while !path.as_ref().starts_with(&common_ancestor) {
                if !common_ancestor.pop() {
                    return Err(ClaudeError::Workspace(
                        "No common ancestor found".to_string(),
                    ));
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
        Err(ClaudeError::Workspace("Cargo.toml not found".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_dummy_project(temp_dir: &Path) -> std::io::Result<()> {
        // Create workspace Cargo.toml
        fs::write(
            temp_dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate1\", \"crate2\"]",
        )?;

        // Create crate1
        fs::create_dir(temp_dir.join("crate1"))?;
        fs::write(
            temp_dir.join("crate1/Cargo.toml"),
            "[package]\nname = \"crate1\"\nversion = \"0.1.0\"",
        )?;
        fs::create_dir(temp_dir.join("crate1/src"))?;
        fs::write(temp_dir.join("crate1/src/lib.rs"), "// Dummy content")?;

        // Create crate2
        fs::create_dir(temp_dir.join("crate2"))?;
        fs::write(
            temp_dir.join("crate2/Cargo.toml"),
            "[package]\nname = \"crate2\"\nversion = \"0.1.0\"",
        )?;
        fs::create_dir(temp_dir.join("crate2/src"))?;
        fs::write(temp_dir.join("crate2/src/lib.rs"), "// Dummy content")?;

        Ok(())
    }

    #[test]
    fn test_discover_workspace() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];

        let workspace = Workspace::discover(&paths)?;

        assert_eq!(workspace.manifest_path, temp_dir.path().join("Cargo.toml"));

        env::set_current_dir(original_dir).unwrap();
        Ok(())
    }

    #[test]
    fn test_discover_single_crate() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let paths = vec![temp_dir.path().join("crate1/src/lib.rs")];

        let workspace = Workspace::discover(&paths)?;

        assert_eq!(
            workspace.manifest_path,
            temp_dir.path().join("crate1/Cargo.toml")
        );

        env::set_current_dir(original_dir).unwrap();
        Ok(())
    }

    #[test]
    fn test_no_cargo_toml() {
        let temp_dir = TempDir::new().unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let paths = vec![temp_dir.path().to_path_buf()];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .ends_with("Cargo.toml not found"),);

        env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_no_paths_provided() {
        let paths: Vec<PathBuf> = vec![];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .ends_with("No paths provided"),);
    }

    #[test]
    fn test_no_common_ancestor() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let paths = vec![
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());
    }
}
