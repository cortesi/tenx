use std::{
    path::Path,
    process::{Command, ExitStatus},
};

use crate::{Result, TenxError};

/// Execute a shell command and return status, stdout and stderr, with ANSI escapes removed.
/// The command is run in the specified root directory.
pub fn exec<P: AsRef<Path>>(root: P, cmd: &str) -> Result<(ExitStatus, String, String)> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(root)
        .output()
        .map_err(|e| TenxError::Exec {
            cmd: cmd.to_string(),
            error: e.to_string(),
        })?;

    let stdo_bytes = strip_ansi_escapes::strip(&output.stdout);
    let stde_bytes = strip_ansi_escapes::strip(&output.stderr);

    let stdout = String::from_utf8_lossy(&stdo_bytes).trim().to_string();
    let stderr = String::from_utf8_lossy(&stde_bytes).trim().to_string();

    Ok((output.status, stdout, stderr))
}
