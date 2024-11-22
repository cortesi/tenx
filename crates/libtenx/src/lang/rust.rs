use std::path::{Path, PathBuf};
use std::process::Command;

use crate::formatters::Formatter;
use crate::{config::Config, Result, Session, TenxError};

use crate::checks::Runnable;
pub struct RustCargoCheck;
pub struct RustCargoTest;
pub struct RustCargoClippy;
pub struct CargoFormatter;

impl Formatter for CargoFormatter {
    fn name(&self) -> &'static str {
        "rust: cargo fmt"
    }

    fn format(&self, config: &Config, state: &Session) -> Result<()> {
        run_cargo_command(config, self.name(), state, &["fmt", "--all"])
    }

    fn is_relevant(&self, config: &Config, state: &Session) -> Result<bool> {
        should_run_rust_check(config, state)
    }

    fn is_configured(&self, config: &Config) -> bool {
        config.formatters.rust_cargo_fmt
    }

    fn runnable(&self) -> Result<Runnable> {
        Ok(Runnable::Ok)
    }
}

fn should_run_rust_check(config: &Config, state: &Session) -> Result<bool> {
    let editables = state.abs_editables(config)?;
    if !editables.is_empty() {
        Ok(editables
            .iter()
            .any(|path| path.extension().map_or(false, |ext| ext == "rs")))
    } else {
        Ok(config
            .included_files()?
            .iter()
            .any(|path| path.extension().map_or(false, |ext| ext == "rs")))
    }
}

fn run_cargo_command(config: &Config, name: &str, state: &Session, args: &[&str]) -> Result<()> {
    let workspace = RustWorkspace::discover(config, state)?;
    let output = Command::new("cargo")
        .args(args)
        .current_dir(&workspace.root_path)
        .output()
        .map_err(|e| TenxError::Check {
            name: name.to_string(),
            user: format!("Failed to execute cargo command: {}", e),
            model: e.to_string(),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if args[0] == "clippy" && !stderr.is_empty() {
        Err(TenxError::Check {
            name: name.to_string(),
            user: "cargo clippy found issues".to_string(),
            model: format!("stderr:\n{}", stderr),
        })
    } else if !output.status.success() {
        Err(TenxError::Check {
            name: name.to_string(),
            user: format!("cargo {} failed", args[0]),
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
    pub fn discover(config: &Config, session: &Session) -> Result<Self> {
        let editables = session.abs_editables(config)?;
        let paths = if editables.is_empty() {
            let included = config.included_files()?;
            if included.is_empty() {
                return Err(TenxError::Workspace("No files to check".to_string()));
            }
            included
        } else {
            editables.clone()
        };

        let common_ancestor = Self::find_common_ancestor(&paths)?;
        let root_path = if editables.is_empty() {
            // With included files, find the outermost workspace
            Self::find_outermost_workspace(config)?
        } else {
            // With editables, find the innermost workspace
            Self::find_workspace_root(&common_ancestor)?
        };

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

    fn find_outermost_workspace(config: &Config) -> Result<PathBuf> {
        let mut last_workspace: Option<PathBuf> = None;
        for path in config.included_files()? {
            let abs_path = config.abspath(&path)?;
            let mut current = abs_path;
            while let Some(parent) = current.parent() {
                let cargo_toml = parent.join("Cargo.toml");
                if cargo_toml.exists() {
                    match &last_workspace {
                        Some(last) => {
                            // Keep the workspace that's higher up in the tree
                            if parent.components().count() < last.components().count() {
                                last_workspace = Some(parent.to_path_buf());
                            }
                        }
                        None => last_workspace = Some(parent.to_path_buf()),
                    }
                }
                current = parent.to_path_buf();
            }
        }

        last_workspace.ok_or_else(|| TenxError::Workspace("Workspace root not found".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{prompt::Prompt, testutils::create_dummy_project};
    use tempfile::TempDir;

    #[test]
    fn test_discover_workspace() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default().with_root(temp_dir.path());
        create_dummy_project(temp_dir.path()).unwrap();

        let edit_paths = vec![
            temp_dir.path().join("crate1/src/lib.rs"),
            temp_dir.path().join("crate2/src/lib.rs"),
        ];

        let prompt = Prompt::User(String::new());

        let mut session = Session::default();
        session.add_prompt(prompt)?;
        for p in edit_paths {
            session.add_editable_path(&config, &p)?;
        }

        let workspace = RustWorkspace::discover(&config, &session)?;
        assert_eq!(
            workspace.root_path.canonicalize().unwrap(),
            temp_dir.path().canonicalize().unwrap()
        );

        Ok(())
    }

    #[test]
    fn test_discover_single_crate() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default().with_root(temp_dir.path());
        create_dummy_project(temp_dir.path()).unwrap();

        let edit_paths = vec![temp_dir.path().join("crate1/src/lib.rs")];

        let prompt = Prompt::User(String::new());

        let mut session = Session::default();
        session.add_prompt(prompt)?;
        for p in edit_paths {
            session.add_editable_path(&config, &p)?;
        }

        let workspace = RustWorkspace::discover(&config, &session)?;

        assert_eq!(
            workspace.root_path.canonicalize().unwrap(),
            temp_dir.path().join("crate1").canonicalize().unwrap()
        );

        Ok(())
    }

    #[test]
    fn test_no_cargo_toml() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default().with_root(temp_dir.path());

        let prompt = Prompt::User(String::new());

        let mut session = Session::default();
        session.add_prompt(prompt)?;
        session.add_editable_path(&config, temp_dir.path())?;

        let result = RustWorkspace::discover(&config, &session);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().ends_with("root not found"));

        Ok(())
    }

    #[test]
    fn test_no_paths_provided() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let mut config = Config::default().with_root(temp_dir.path());

        // Make sure we don't try to use git
        use crate::config::Include;
        config.include = Include::Glob(vec![]);

        let prompt = Prompt::default();

        let mut session = Session::default();
        session.add_prompt(prompt)?;

        let result = RustWorkspace::discover(&config, &session);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        println!("Error message: {}", err);
        assert!(err.ends_with("No files to check"));

        Ok(())
    }

    #[test]
    fn test_no_common_ancestor() -> Result<()> {
        let temp_dir1 = TempDir::new().unwrap();
        let config = Config::default().with_root(temp_dir1.path());

        let temp_dir2 = TempDir::new().unwrap();

        let edit_paths = vec![
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ];

        let prompt = Prompt::User(String::new());

        let mut session = Session::default();
        session.add_prompt(prompt)?;
        for f in edit_paths {
            session.add_editable_path(&config, &f)?;
        }

        let result = RustWorkspace::discover(&config, &session);

        assert!(result.is_err());

        Ok(())
    }
}
