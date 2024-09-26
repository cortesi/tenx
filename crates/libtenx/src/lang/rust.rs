use std::path::{Path, PathBuf};
use std::process::Command;

use crate::formatters::Formatter;
use crate::validators::{Runnable, Validator};
use crate::{config::Config, Result, Session, TenxError};

pub struct RustCargoCheck;
pub struct RustCargoTest;
pub struct RustCargoClippy;
pub struct CargoFormatter;

fn cargo_runnable() -> Result<Runnable> {
    if is_cargo_installed() {
        Ok(Runnable::Ok)
    } else {
        Ok(Runnable::Error("Cargo is not installed".to_string()))
    }
}

impl Validator for RustCargoCheck {
    fn name(&self) -> &'static str {
        "rust: cargo check"
    }

    fn validate(&self, state: &Session) -> Result<()> {
        run_cargo_command(self.name(), state, &["check", "--tests"])
    }

    fn is_relevant(&self, _config: &Config, state: &Session) -> Result<bool> {
        should_run_rust_validator(state)
    }

    fn is_configured(&self, config: &Config) -> bool {
        config.validators.rust_cargo_check
    }

    fn runnable(&self) -> Result<Runnable> {
        cargo_runnable()
    }
}

impl Validator for RustCargoTest {
    fn name(&self) -> &'static str {
        "rust: cargo test"
    }

    fn validate(&self, state: &Session) -> Result<()> {
        run_cargo_command(self.name(), state, &["test", "-q"])
    }

    fn is_relevant(&self, _config: &Config, state: &Session) -> Result<bool> {
        should_run_rust_validator(state)
    }

    fn is_configured(&self, config: &Config) -> bool {
        config.validators.rust_cargo_test
    }

    fn runnable(&self) -> Result<Runnable> {
        cargo_runnable()
    }
}

impl Validator for RustCargoClippy {
    fn name(&self) -> &'static str {
        "rust: cargo clippy"
    }

    fn validate(&self, state: &Session) -> Result<()> {
        run_cargo_command(
            self.name(),
            state,
            &["clippy", "--no-deps", "--all", "--tests", "-q"],
        )
    }

    fn is_relevant(&self, _config: &Config, state: &Session) -> Result<bool> {
        should_run_rust_validator(state)
    }

    fn is_configured(&self, config: &Config) -> bool {
        config.validators.rust_cargo_clippy
    }

    fn runnable(&self) -> Result<Runnable> {
        cargo_runnable()
    }
}

impl Formatter for CargoFormatter {
    fn name(&self) -> &'static str {
        "cargo fmt"
    }

    fn format(&self, state: &Session) -> Result<()> {
        run_cargo_command(self.name(), state, &["fmt", "--all"])
    }
}

fn should_run_rust_validator(state: &Session) -> Result<bool> {
    Ok(state
        .abs_editables()?
        .iter()
        .any(|path| path.extension().map_or(false, |ext| ext == "rs")))
}

fn is_cargo_installed() -> bool {
    Command::new("cargo")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if args[0] == "clippy" && !stderr.is_empty() {
        Err(TenxError::Validation {
            name: name.to_string(),
            user: "Cargo clippy found issues".to_string(),
            model: format!("stderr:\n{}", stderr),
        })
    } else if !output.status.success() {
        Err(TenxError::Validation {
            name: name.to_string(),
            user: format!("Cargo {} failed", args[0]),
            model: format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr),
        })
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub struct RustWorkspace {
    pub root_path: PathBuf,
}

impl RustWorkspace {
    pub fn discover(session: &Session) -> Result<Self> {
        let common_ancestor = Self::find_common_ancestor(&session.abs_editables()?)?;
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
    use crate::{prompt::Prompt, testutils::create_dummy_project};
    use tempfile::TempDir;

    #[test]
    fn test_cargo_checker() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();

        let edit_paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];
        let prompt = Prompt::User(String::new());

        let mut session = Session::new(temp_dir.path().to_path_buf());
        session.add_prompt(prompt.clone())?;
        for p in edit_paths {
            session.add_editable_path(&p)?;
        }

        let checker = RustCargoCheck;
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

        let prompt = Prompt::User(String::new());

        let mut session = Session::new(temp_dir.path().to_path_buf());
        session.add_prompt(prompt)?;
        for p in edit_paths {
            session.add_editable_path(&p)?;
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

        let prompt = Prompt::User(String::new());

        let mut session = Session::new(temp_dir.path().to_path_buf());
        session.add_prompt(prompt)?;
        for p in edit_paths {
            session.add_editable_path(&p)?;
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

        let prompt = Prompt::User(String::new());

        let mut session = Session::new(temp_dir.path().to_path_buf());
        session.add_prompt(prompt)?;
        session.add_editable_path(temp_dir.path())?;

        let result = RustWorkspace::discover(&session);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().ends_with("root not found"));

        Ok(())
    }

    #[test]
    fn test_no_paths_provided() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        let prompt = Prompt::default();

        let mut session = Session::new(temp_dir.path().to_path_buf());
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

        let prompt = Prompt::User(String::new());

        let mut session = Session::new(temp_dir1.path().to_path_buf());
        session.add_prompt(prompt)?;
        for f in edit_paths {
            session.add_editable_path(&f)?;
        }

        let result = RustWorkspace::discover(&session);

        assert!(result.is_err());

        Ok(())
    }
}
