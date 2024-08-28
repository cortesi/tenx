use std::path::{Path, PathBuf};
use std::process::Command;

use super::Validator;
use crate::{Result, Session, TenxError};

pub struct CargoChecker;
pub struct CargoTester;

impl Validator for CargoChecker {
    fn name(&self) -> &'static str {
        "CargoChecker"
    }

    fn validate(&self, state: &Session) -> Result<()> {
        run_cargo_command(self.name(), state, &["check", "--tests"])
    }
}

impl Validator for CargoTester {
    fn name(&self) -> &'static str {
        "CargoTester"
    }

    fn validate(&self, state: &Session) -> Result<()> {
        run_cargo_command(self.name(), state, &["test"])
    }
}

fn run_cargo_command(name: &str, state: &Session, args: &[&str]) -> Result<()> {
    let workspace = RustWorkspace::discover(state)?;
    let output = Command::new("cargo")
        .args(args)
        .current_dir(&workspace.root_path)
        .output()
        .map_err(|e| TenxError::Validation {
            name: name.to_string(),
            user: format!("Failed to execute cargo command: {}", e),
            model: e.to_string(),
        })?;

    if output.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(TenxError::Validation {
            name: name.to_string(),
            user: format!("Cargo {} failed", args[0]),
            model: format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr),
        })
    }
}

#[derive(Debug)]
pub struct RustWorkspace {
    pub root_path: PathBuf,
}

impl RustWorkspace {
    pub fn discover(session: &Session) -> Result<Self> {
        let common_ancestor = Self::find_common_ancestor(&session.editables()?)?;
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
        Err(TenxError::Workspace("Workspace root not found".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialect::Dialect, model::Model, prompt::Prompt, testutils::create_dummy_project};
    use tempfile::TempDir;

    #[test]
    fn test_cargo_checker() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let edit_paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];
        let prompt = Prompt {
            ..Default::default()
        };

        let mut session = Session::new(
            temp_dir.path().to_path_buf(),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::DummyModel::default()),
        );
        session.add_prompt(prompt.clone())?;
        for p in edit_paths {
            session.add_editable(&p)?;
        }

        let checker = CargoChecker;
        assert!(checker.validate(&session).is_ok());

        Ok(())
    }

    #[test]
    fn test_discover_workspace() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let edit_paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];

        let prompt = Prompt {
            ..Default::default()
        };

        let mut session = Session::new(
            temp_dir.path().to_path_buf(),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::DummyModel::default()),
        );
        session.add_prompt(prompt)?;
        for p in edit_paths {
            session.add_editable(&p)?;
        }

        let workspace = RustWorkspace::discover(&session)?;
        assert_eq!(
            workspace.root_path.canonicalize().unwrap(),
            temp_dir.path().canonicalize().unwrap()
        );

        Ok(())
    }

    #[test]
    fn test_discover_single_crate() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let edit_paths = vec![temp_dir.path().join("crate1/src/lib.rs")];

        let prompt = Prompt {
            ..Default::default()
        };

        let mut session = Session::new(
            temp_dir.path().to_path_buf(),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::DummyModel::default()),
        );
        session.add_prompt(prompt)?;
        for p in edit_paths {
            session.add_editable(&p)?;
        }

        let workspace = RustWorkspace::discover(&session)?;

        assert_eq!(
            workspace.root_path.canonicalize().unwrap(),
            temp_dir.path().join("crate1").canonicalize().unwrap()
        );

        Ok(())
    }

    #[test]
    fn test_no_cargo_toml() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        let prompt = Prompt {
            ..Default::default()
        };

        let mut session = Session::new(
            temp_dir.path().to_path_buf(),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::DummyModel::default()),
        );
        session.add_prompt(prompt)?;
        session.add_editable(temp_dir.path())?;

        let result = RustWorkspace::discover(&session);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().ends_with("root not found"));

        Ok(())
    }

    #[test]
    fn test_no_paths_provided() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        let prompt = Prompt::default();

        let mut session = Session::new(
            temp_dir.path().to_path_buf(),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::DummyModel::default()),
        );
        session.add_prompt(prompt)?;

        let result = RustWorkspace::discover(&session);

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

        let edit_paths = vec![
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ];

        let prompt = Prompt {
            ..Default::default()
        };

        let mut session = Session::new(
            temp_dir1.path().to_path_buf(),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::DummyModel::default()),
        );
        session.add_prompt(prompt)?;
        for f in edit_paths {
            session.add_editable(&f)?;
        }

        let result = RustWorkspace::discover(&session);

        assert!(result.is_err());

        Ok(())
    }
}
