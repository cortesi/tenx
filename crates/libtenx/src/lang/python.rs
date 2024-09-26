use std::path::Path;
use std::process::Command;

use crate::formatters::Formatter;
use crate::validators::{Runnable, Validator};
use crate::{config::Config, Result, Session, TenxError};

pub struct PythonRuffCheck;
pub struct PythonRuffFormatter;

impl Validator for PythonRuffCheck {
    fn name(&self) -> &'static str {
        "python: ruff check"
    }

    fn validate(&self, state: &Session) -> Result<()> {
        for file in state.abs_editables()? {
            if file.extension().map_or(false, |ext| ext == "py") {
                run_ruff_check(&file)?;
            }
        }
        Ok(())
    }

    fn is_relevant(&self, _config: &Config, state: &Session) -> Result<bool> {
        Ok(state
            .abs_editables()?
            .iter()
            .any(|path| path.extension().map_or(false, |ext| ext == "py")))
    }

    fn is_configured(&self, config: &Config) -> bool {
        config.validators.python_ruff_check
    }

    fn runnable(&self) -> Result<Runnable> {
        if is_ruff_installed() {
            Ok(Runnable::Ok)
        } else {
            Ok(Runnable::Error("ruff is not installed".to_string()))
        }
    }
}

impl Formatter for PythonRuffFormatter {
    fn name(&self) -> &'static str {
        "python: ruff format"
    }

    fn format(&self, state: &Session) -> Result<()> {
        for file in state.abs_editables()? {
            if file.extension().map_or(false, |ext| ext == "py") {
                run_ruff_format(&file)?;
            }
        }
        Ok(())
    }

    fn is_relevant(&self, _config: &Config, state: &Session) -> Result<bool> {
        Ok(state
            .abs_editables()?
            .iter()
            .any(|path| path.extension().map_or(false, |ext| ext == "py")))
    }

    fn is_configured(&self, config: &Config) -> bool {
        config.formatters.python_ruff_fmt
    }

    fn runnable(&self) -> Result<Runnable> {
        if is_ruff_installed() {
            Ok(Runnable::Ok)
        } else {
            Ok(Runnable::Error("ruff is not installed".to_string()))
        }
    }
}

fn is_ruff_installed() -> bool {
    Command::new("ruff")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_ruff_check(file_path: &Path) -> Result<()> {
    let output = Command::new("ruff")
        .args(["check", "-q"])
        .arg(file_path)
        .output()
        .map_err(|e| TenxError::Validation {
            name: "python: ruff check".to_string(),
            user: format!("Failed to execute ruff command: {}", e),
            model: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(TenxError::Validation {
            name: "python: ruff check".to_string(),
            user: "Ruff found issues".to_string(),
            model: format!("stderr:\n{}", stderr),
        })
    } else {
        Ok(())
    }
}

fn run_ruff_format(file_path: &Path) -> Result<()> {
    let output = Command::new("ruff")
        .args(["format"])
        .arg(file_path)
        .output()
        .map_err(|e| TenxError::Validation {
            name: "python: ruff format".to_string(),
            user: format!("Failed to execute ruff format command: {}", e),
            model: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(TenxError::Validation {
            name: "python: ruff format".to_string(),
            user: "Ruff format failed".to_string(),
            model: format!("stderr:\n{}", stderr),
        })
    } else {
        Ok(())
    }
}
