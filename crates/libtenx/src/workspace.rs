use crate::error::{ClaudeError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Workspace {
    root_path: PathBuf,
}

impl Workspace {
    pub fn discover<P: AsRef<Path>>(paths: &[P]) -> Result<Self> {
        let common_ancestor = Self::find_common_ancestor(paths)?;
        let root_path = Self::find_workspace_root(&common_ancestor)?;

        // Ensure root_path is absolute
        let root_path = if root_path.is_absolute() {
            root_path
        } else {
            std::env::current_dir()?.join(root_path)
        };

        Ok(Workspace { root_path })
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

    fn find_workspace_root(start_dir: &Path) -> Result<PathBuf> {
        let mut current_dir = start_dir.to_path_buf();
        loop {
            let cargo_toml = current_dir.join("Cargo.toml");
            if cargo_toml.exists() {
                return Ok(current_dir);
            }
            if !current_dir.pop() {
                break;
            }
        }
        Err(ClaudeError::Workspace(
            "Workspace root not found".to_string(),
        ))
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root_path.join("Cargo.toml")
    }

    pub fn relative_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        let path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };

        path.strip_prefix(&self.root_path)
            .map(|p| p.to_path_buf())
            .map_err(|e| ClaudeError::Workspace(format!("Failed to get relative path: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    struct TempEnv {
        original_dir: PathBuf,
    }

    impl TempEnv {
        fn new<P: AsRef<Path>>(temp_dir: P) -> std::io::Result<Self> {
            let original_dir = env::current_dir()?;
            env::set_current_dir(temp_dir)?;
            Ok(TempEnv { original_dir })
        }
    }

    impl Drop for TempEnv {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original_dir);
        }
    }

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

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];

        let workspace = Workspace::discover(&paths)?;

        assert_eq!(
            workspace.manifest_path(),
            temp_dir.path().join("Cargo.toml")
        );

        Ok(())
    }

    #[test]
    fn test_discover_single_crate() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().join("crate1/src/lib.rs")];

        let workspace = Workspace::discover(&paths)?;

        assert_eq!(
            workspace.manifest_path(),
            temp_dir.path().join("crate1/Cargo.toml")
        );

        Ok(())
    }

    #[test]
    fn test_no_cargo_toml() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths = vec![temp_dir.path().to_path_buf()];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().ends_with("root not found"));

        Ok(())
    }

    #[test]
    fn test_no_paths_provided() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let _temp_env = TempEnv::new(temp_dir.path())?;

        let paths: Vec<PathBuf> = vec![];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .ends_with("No paths provided"));

        Ok(())
    }

    #[test]
    fn test_no_common_ancestor() -> Result<()> {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let _temp_env = TempEnv::new(&temp_dir1)?;

        let paths = vec![
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ];

        let result = Workspace::discover(&paths);

        assert!(result.is_err());

        Ok(())
    }
}
