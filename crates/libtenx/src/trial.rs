//! Trial module for defining and running test trials.
//!
//! A trial consists of a TrialConf at NAME.toml which specifies the operations to perform,
//! and an optional Tenx Config at NAME.conf.toml which overrides the default configuration.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fs_extra;
use serde::Deserialize;
use tempfile::TempDir;
use tracing::info;

use crate::{config::Config, Result, TenxError};

#[derive(Debug, Clone, Deserialize)]
pub struct Edit {
    pub prompt: String,
    pub edit: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialOp {
    Edit(Edit),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrialConf {
    pub project: String,
    pub op: TrialOp,
}

impl TrialConf {
    /// Parse a TOML string into a TrialConf
    fn from_str(s: &str) -> Result<Self> {
        toml::from_str(s)
            .map_err(|e| TenxError::Internal(format!("Failed to parse trial TOML: {}", e)))
    }

    /// Read a trial configuration from a TOML file
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .map_err(|e| TenxError::Internal(format!("Failed to read trial file: {}", e)))?;
        Self::from_str(&contents)
    }

    /// Validates that the trial configuration is valid for the given base directory
    pub fn validate<P: AsRef<Path>>(&self, base_dir: P) -> Result<()> {
        let mut project_path = PathBuf::from(base_dir.as_ref());
        project_path.push("projects");
        project_path.push(&self.project);

        if !project_path.exists() {
            return Err(TenxError::Internal(format!(
                "Project directory '{}' does not exist",
                project_path.display()
            )));
        }
        Ok(())
    }
}

pub struct Trial {
    pub name: String,
    pub base_dir: PathBuf,
    pub trial_conf: TrialConf,
    pub tenx_conf: Config,
}

impl Trial {
    /// Creates a temporary directory and copies the project into it
    fn setup_temp_project(&self) -> Result<TempDir> {
        let temp_dir = TempDir::new()
            .map_err(|e| TenxError::Internal(format!("Failed to create temp directory: {}", e)))?;

        let mut src_path = self.base_dir.clone();
        src_path.push("projects");
        src_path.push(&self.trial_conf.project);

        let dst_path = temp_dir.path().to_path_buf();

        fs_extra::dir::copy(&src_path, &dst_path, &fs_extra::dir::CopyOptions::new())
            .map_err(|e| TenxError::Internal(format!("Failed to copy project directory: {}", e)))?;

        Ok(temp_dir)
    }

    /// Executes the trial in a temporary directory
    pub fn execute(&self) -> Result<()> {
        let _temp_dir = self.setup_temp_project()?;
        info!("trial setup complete");
        Ok(())
    }

    /// Returns a default configuration for trials
    fn default_config() -> Result<Config> {
        let mut config = Config::default();
        config.include = crate::config::Include::Glob(vec!["**/*".to_string()]);
        Ok(config)
    }

    /// Loads a trial from the base directory with the specified name
    pub fn load<P: AsRef<Path>>(base_dir: P, name: &str) -> Result<Self> {
        info!("loading trial: {}", name);
        let mut path = PathBuf::from(base_dir.as_ref());
        path.push(format!("{}.toml", name));
        let trial_conf = TrialConf::read(&path)?;
        trial_conf.validate(&base_dir)?;

        path.set_file_name(format!("{}.conf.toml", name));
        let tenx_conf = if path.exists() {
            let contents = fs::read_to_string(&path)
                .map_err(|e| TenxError::Internal(format!("Failed to read config file: {}", e)))?;
            toml::from_str(&contents)
                .map_err(|e| TenxError::Internal(format!("Failed to parse config TOML: {}", e)))?
        } else {
            Self::default_config()?
        };

        Ok(Trial {
            name: name.to_string(),
            base_dir: base_dir.as_ref().to_path_buf(),
            trial_conf,
            tenx_conf,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trial_conf_from_str() -> Result<()> {
        let toml = r#"
            project = "test_project"
            [op.edit]
            prompt = "test prompt"
            edit = ["file1.rs", "file2.rs"]
        "#;

        let conf = TrialConf::from_str(toml)?;
        assert_eq!(conf.project, "test_project");

        match conf.op {
            TrialOp::Edit(edit) => {
                assert_eq!(edit.prompt, "test prompt");
                assert_eq!(
                    edit.edit,
                    vec![PathBuf::from("file1.rs"), PathBuf::from("file2.rs")]
                );
            }
        }

        Ok(())
    }
}
