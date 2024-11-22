use glob::glob;
use std::process::Command;

use crate::{
    config::Config,
    validators::{Runnable, Validator},
    Result, Session, TenxError,
};

pub struct Shell {
    pub name: String,
    pub command: String,
    pub globs: Vec<String>,
    pub default_on: bool,
}

impl Validator for Shell {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn validate(&self, config: &Config, _state: &Session) -> Result<()> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .current_dir(config.project_root())
            .output()
            .map_err(|e| TenxError::Validation {
                name: self.name.clone(),
                user: format!("Failed to execute shell command: {}", e),
                model: e.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            Err(TenxError::Validation {
                name: self.name.clone(),
                user: format!("Shell command failed: {}", self.command),
                model: format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr),
            })
        } else {
            Ok(())
        }
    }

    fn is_relevant(&self, config: &Config, state: &Session) -> Result<bool> {
        let editables = state.abs_editables(config)?;
        if editables.is_empty() {
            return Ok(self.default_on);
        }

        for editable in editables {
            for pattern in &self.globs {
                if let Ok(paths) = glob(pattern) {
                    if paths
                        .into_iter()
                        .any(|p| p.ok().map_or(false, |p| p == editable))
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn is_configured(&self, _config: &Config) -> bool {
        true
    }

    fn runnable(&self) -> Result<Runnable> {
        Ok(Runnable::Ok)
    }
}
