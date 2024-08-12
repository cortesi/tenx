use std::path::{Path, PathBuf};
use std::process::Command;

use super::Validator;
use crate::{PromptInput, Result, State, TenxError};

pub struct CargoChecker;

impl Validator for CargoChecker {
    fn validate(&self, prompt: &PromptInput, state: &State) -> Result<()> {
        let workspace = RustWorkspace::discover(prompt, state)?;
        let output = Command::new("cargo")
            .arg("check")
            .current_dir(&workspace.root_path)
            .output()
            .map_err(|e| TenxError::Workspace(format!("Failed to execute cargo check: {}", e)))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(TenxError::Workspace(format!(
                "Cargo check failed: {}",
                stderr
            )))
        }
    }
}

#[derive(Debug)]
struct RustWorkspace {
    root_path: PathBuf,
}

impl RustWorkspace {
    pub fn discover(prompt: &PromptInput, state: &State) -> Result<Self> {
        let paths: Vec<PathBuf> = prompt
            .edit_paths
            .iter()
            .chain(state.snapshot.keys())
            .cloned()
            .collect();
        let common_ancestor = Self::find_common_ancestor(&paths)?;
        let root_path = Self::find_workspace_root(&common_ancestor)?;

        Ok(RustWorkspace { root_path })
    }

    fn find_common_ancestor<P: AsRef<Path>>(paths: &[P]) -> Result<PathBuf> {
        if paths.is_empty() {
            return Err(TenxError::Workspace("No paths provided".to_string()));
        }

        let mut common_ancestor = paths[0].as_ref().to_path_buf();
        for path in paths.iter().skip(1) {
            while !path.as_ref().starts_with(&common_ancestor) {
                if !common_ancestor.pop() {
                    return Err(TenxError::Workspace("No common ancestor found".to_string()));
                }
            }
        }

        Ok(common_ancestor)
    }

    fn find_workspace_root(start_dir: &Path) -> Result<PathBuf> {
        let mut current_dir = if start_dir.is_absolute() {
            start_dir.to_path_buf()
        } else {
            std::env::current_dir()?.join(start_dir)
        };

        loop {
            let cargo_toml = current_dir.join("Cargo.toml");
            if cargo_toml.exists() {
                return Ok(current_dir);
            }
            if !current_dir.pop() {
                break;
            }
        }
        Err(TenxError::Workspace("Workspace root not found".to_string()))
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root_path.join("Cargo.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::Dialect;
    use crate::model::Model;
    use crate::testutils::{create_dummy_project, TempEnv};
    use tempfile::TempDir;

    #[test]
    fn test_cargo_checker() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let state = State {
            working_directory: temp_dir.path().to_path_buf(),
            model: Model::Dummy(crate::model::Dummy::default()),
            dialect: Dialect::Tags(crate::dialect::Tags::default()),
            snapshot: std::collections::HashMap::new(),
        };

        let prompt = PromptInput {
            edit_paths: vec![
                temp_dir.path().join("crate1/src/lib.rs"),
                temp_dir.path().join("crate2/src/lib.rs"),
            ],
            ..Default::default()
        };

        let checker = CargoChecker;
        assert!(checker.validate(&prompt, &state).is_ok());

        Ok(())
    }

    #[test]
    fn test_discover_workspace() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let _temp_env = TempEnv::new(temp_dir.path())?;

        let prompt = PromptInput {
            edit_paths: vec![
                temp_dir.path().join("crate1/src/lib.rs"),
                temp_dir.path().join("crate2/src/lib.rs"),
            ],
            ..Default::default()
        };

        let state = State {
            working_directory: temp_dir.path().to_path_buf(),
            model: Model::Dummy(crate::model::Dummy::default()),
            dialect: Dialect::Tags(crate::dialect::Tags::default()),
            snapshot: std::collections::HashMap::new(),
        };

        let workspace = RustWorkspace::discover(&prompt, &state)?;

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

        let prompt = PromptInput {
            edit_paths: vec![temp_dir.path().join("crate1/src/lib.rs")],
            ..Default::default()
        };

        let state = State {
            working_directory: temp_dir.path().to_path_buf(),
            model: Model::Dummy(crate::model::Dummy::default()),
            dialect: Dialect::Tags(crate::dialect::Tags::default()),
            snapshot: std::collections::HashMap::new(),
        };

        let workspace = RustWorkspace::discover(&prompt, &state)?;

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

        let prompt = PromptInput {
            edit_paths: vec![temp_dir.path().to_path_buf()],
            ..Default::default()
        };

        let state = State {
            working_directory: temp_dir.path().to_path_buf(),
            model: Model::Dummy(crate::model::Dummy::default()),
            dialect: Dialect::Tags(crate::dialect::Tags::default()),
            snapshot: std::collections::HashMap::new(),
        };

        let result = RustWorkspace::discover(&prompt, &state);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().ends_with("root not found"));

        Ok(())
    }

    #[test]
    fn test_no_paths_provided() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let _temp_env = TempEnv::new(temp_dir.path())?;

        let prompt = PromptInput::default();

        let state = State {
            working_directory: temp_dir.path().to_path_buf(),
            model: Model::Dummy(crate::model::Dummy::default()),
            dialect: Dialect::Tags(crate::dialect::Tags::default()),
            snapshot: std::collections::HashMap::new(),
        };

        let result = RustWorkspace::discover(&prompt, &state);

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

        let prompt = PromptInput {
            edit_paths: vec![
                temp_dir1.path().to_path_buf(),
                temp_dir2.path().to_path_buf(),
            ],
            ..Default::default()
        };

        let state = State {
            working_directory: temp_dir1.path().to_path_buf(),
            model: Model::Dummy(crate::model::Dummy::default()),
            dialect: Dialect::Tags(crate::dialect::Tags::default()),
            snapshot: std::collections::HashMap::new(),
        };

        let result = RustWorkspace::discover(&prompt, &state);

        assert!(result.is_err());

        Ok(())
    }
}
