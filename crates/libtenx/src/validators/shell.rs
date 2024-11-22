use std::process::Command;

use crate::{
    config::Config,
    validators::{Runnable, Validator},
    Result, Session, TenxError,
};

/// A validator that runs a shell command and checks its output. Relies on `sh` being available.
///
/// Validator commands are always run in the project root directory.
pub struct Shell {
    /// Name of the validator for display and error reporting
    pub name: String,
    /// Shell command to execute, run with sh -c
    pub command: String,
    /// List of glob patterns to match against files for determining relevance
    pub globs: Vec<String>,
    /// Whether this validator defaults to off in the configuration
    pub default_off: bool,
    /// Whether to treat any stderr output as a failure, regardless of exit code
    pub fail_on_stderr: bool,
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
            .map_err(|e| TenxError::Internal(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() || (self.fail_on_stderr && !stderr.is_empty()) {
            let msg = format!("Validator command failed: {}", self.command);
            Err(TenxError::Validation {
                name: self.name.clone(),
                user: msg,
                model: format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr),
            })
        } else {
            Ok(())
        }
    }

    fn is_relevant(&self, _config: &Config, state: &Session) -> Result<bool> {
        if state.editable().is_empty() {
            eprintln!("No editables");
            return Ok(false);
        }

        for editable in state.editable() {
            let path_str = editable.to_str().unwrap_or_default();
            eprintln!("Checking editable path: {}", path_str);

            for pattern in &self.globs {
                eprintln!("Against pattern: {}", pattern);
                let glob_pattern =
                    glob::Pattern::new(pattern).map_err(|e| TenxError::Internal(e.to_string()))?;

                // Try both with and without leading ./
                let clean_path = path_str.trim_start_matches("./");
                let matches = glob_pattern.matches(path_str) || glob_pattern.matches(clean_path);
                eprintln!(
                    "Clean path: {}, Pattern match result: {}",
                    clean_path, matches
                );

                if matches {
                    return Ok(true);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    fn test_config() -> Config {
        let mut config = Config::default();
        config.project_root = crate::config::ProjectRoot::Path(".".into());
        config
    }

    fn setup_test_session(paths: &[&str]) -> (Config, Session) {
        let config = test_config();
        let mut session = Session::default();
        let editables: Vec<_> = paths.iter().map(|p| std::path::PathBuf::from(p)).collect();
        // Access the private field through serde
        session = serde_json::from_str(&format!(
            r#"{{"editable":{:?},"steps":[],"contexts":[]}}"#,
            editables
        ))
        .expect("Failed to create test session");
        eprintln!("Created session with editables: {:?}", session.editable());
        (config, session)
    }

    #[test]
    fn test_shell_success() {
        let shell = Shell {
            name: "test".to_string(),
            command: "true".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: true,
        };

        let (config, session) = setup_test_session(&[]);
        let result = shell.validate(&config, &session);
        assert!(result.is_ok());
    }

    #[test]
    fn test_shell_failure() {
        let shell = Shell {
            name: "test".to_string(),
            command: "echo 'error message' >&2 && echo 'output message' && false".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: true,
        };

        let (config, session) = setup_test_session(&[]);
        let result = shell.validate(&config, &session);
        assert!(result.is_err());

        match result {
            Err(TenxError::Validation { name, user, model }) => {
                assert_eq!(name, "test");
                assert!(user.contains("Validator command failed"));
                assert!(model.contains("stdout:\noutput message"));
                assert!(model.contains("stderr:\nerror message"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_is_relevant() {
        let shell = Shell {
            name: "test".to_string(),
            command: "true".to_string(),
            globs: vec![
                "*.rs".to_string(),
                "src/*.rs".to_string(),
                "test.py".to_string(),
            ],
            default_off: false,
            fail_on_stderr: true,
        };

        // Test empty session
        let (config, session) = setup_test_session(&[]);
        assert!(!shell.is_relevant(&config, &session).unwrap());

        // Test Rust file in src directory
        let (config, session) = setup_test_session(&["src/main.rs"]);
        assert!(
            shell.is_relevant(&config, &session).unwrap(),
            "src/main.rs should be relevant"
        );

        // Test Python file
        let (config, session) = setup_test_session(&["test.py"]);
        assert!(
            shell.is_relevant(&config, &session).unwrap(),
            "test.py should be relevant"
        );

        // Test non-matching file
        let (config, session) = setup_test_session(&["test.txt"]);
        assert!(!shell.is_relevant(&config, &session).unwrap());
    }
}
