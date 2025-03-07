use std::path::PathBuf;

use crate::{
    config::Config,
    error::{Result, TenxError},
    events::{EventBlock, EventSender},
    exec::exec,
};

pub enum Runnable {
    Ok,
    Error(String),
}

impl Runnable {
    pub fn is_ok(&self) -> bool {
        matches!(self, Runnable::Ok)
    }
}

/// A validator that runs a shell command and checks its output. Relies on `sh` being available.
///
/// Check commands are always run in the project root directory.
pub struct Check {
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

impl Check {
    pub fn check(&self, config: &Config) -> Result<()> {
        let (status, stdout, stderr) = exec(config.project_root(), &self.command)?;

        if !status.success() || (self.fail_on_stderr && !stderr.is_empty()) {
            let msg = format!("Check command failed: {}", self.command);
            Err(TenxError::Check {
                name: self.name.clone(),
                user: msg,
                model: format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr),
            })
        } else {
            Ok(())
        }
    }

    /// Determines if a path matches any of the given glob patterns.
    fn match_globs(&self, path_str: &str, patterns: &[String]) -> Result<bool> {
        for pattern in patterns {
            let glob_pattern =
                glob::Pattern::new(pattern).map_err(|e| TenxError::Internal(e.to_string()))?;
            let clean_path = path_str.trim_start_matches("./");
            if glob_pattern.matches(path_str) || glob_pattern.matches(clean_path) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Is a check relevant based on its glob patterns and the files to check?
    pub fn is_relevant(&self, paths: &Vec<PathBuf>) -> Result<bool> {
        for path in paths {
            let path_str = path.to_str().unwrap_or_default();
            if self.match_globs(path_str, &self.globs)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn runnable(&self) -> Result<Runnable> {
        Ok(Runnable::Ok)
    }

    pub fn default_off(&self) -> bool {
        self.default_off
    }
}

/// Run checks on a given set of paths with a mode filter.
pub fn check_paths(
    conf: &Config,
    paths: &Vec<PathBuf>,
    sender: &Option<EventSender>,
) -> Result<()> {
    for c in conf.enabled_checks() {
        if c.is_relevant(paths)? {
            let _check_block = EventBlock::check(sender, &c.name)?;
            c.check(conf)?;
        }
    }
    Ok(())
}

/// Run checks on all configured state files.
pub fn check_all(conf: &Config, sender: &Option<EventSender>) -> Result<()> {
    let state = conf.state()?;
    check_paths(conf, &state.list()?, sender)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        let mut config = Config::default();
        config.project.root = ".".into();
        config
    }

    #[test]
    fn test_match_globs() {
        let check = Check {
            name: "test".to_string(),
            command: "true".to_string(),
            globs: vec!["src/*.rs".to_string(), "tests/**/*.rs".to_string()],
            default_off: false,
            fail_on_stderr: true,
        };

        let patterns = check.globs.clone();
        assert!(check.match_globs("src/lib.rs", &patterns).unwrap());
        assert!(check.match_globs("tests/unit/check.rs", &patterns).unwrap());
        assert!(!check.match_globs("README.md", &patterns).unwrap());
    }

    #[test]
    fn test_shell_success() {
        let shell = Check {
            name: "test".to_string(),
            command: "true".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: true,
        };

        let config = test_config();
        let result = shell.check(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_shell_failure() {
        let shell = Check {
            name: "test".to_string(),
            command: "echo 'error message' >&2 && echo 'output message' && false".to_string(),
            globs: vec!["*.rs".to_string()],
            default_off: false,
            fail_on_stderr: true,
        };

        let config = test_config();
        let result = shell.check(&config);
        assert!(result.is_err());

        match result {
            Err(TenxError::Check { name, user, model }) => {
                assert_eq!(name, "test");
                assert!(user.contains("Check command failed"));
                assert!(model.contains("stdout:\noutput message"));
                assert!(model.contains("stderr:\nerror message"));
            }
            _ => panic!("Expected Check error"),
        }
    }
}
