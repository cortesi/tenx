use std::process::Command;

use super::Formatter;
use crate::{Result, Session, TenxError};

pub struct CargoFormatter;

impl Formatter for CargoFormatter {
    fn name(&self) -> &'static str {
        "CargoFormatter"
    }

    fn format(&self, state: &Session) -> Result<()> {
        let workspace = crate::validators::rust::RustWorkspace::discover(state)?;
        let output = Command::new("cargo")
            .args(["fmt", "--all"])
            .current_dir(&workspace.root_path)
            .output()
            .map_err(|e| TenxError::Validation {
                name: self.name().to_string(),
                user: format!("Failed to execute cargo fmt: {}", e),
                model: e.to_string(),
            })?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(TenxError::Validation {
                name: self.name().to_string(),
                user: "Cargo fmt failed".to_string(),
                model: stderr.to_string(),
            })
        }
    }
}

