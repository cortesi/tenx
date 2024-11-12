//! Trial module for defining and running test trials.
//!
//! A trial consists of a TrialConf at NAME.toml which specifies the operations to perform, as well
//! as an embedded tenx configuration.

use crate::Event;
use glob::glob;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;

use fs_extra;
use serde::Deserialize;
use tempfile::TempDir;
use tracing::info;

use crate::{
    config::{Config, ProjectRoot},
    Result, Tenx, TenxError,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Ask {
    pub prompt: String,
    pub editable: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Fix {
    pub prompt: Option<String>,
    pub editable: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialOp {
    Ask(Ask),
    Fix(Fix),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrialConf {
    pub project: String,
    pub op: TrialOp,
    pub config: Option<Config>,
    pub desc: Option<String>,
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
    pub desc: String,
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

        let mut dst_path = temp_dir.path().to_path_buf();
        dst_path.push(&self.trial_conf.project);

        let session_path = dst_path.join("session");
        fs::create_dir_all(&session_path).map_err(|e| {
            TenxError::Internal(format!("Failed to create session directory: {}", e))
        })?;

        fs_extra::dir::copy(&src_path, &dst_path, &fs_extra::dir::CopyOptions::new())
            .map_err(|e| TenxError::Internal(format!("Failed to copy project directory: {}", e)))?;

        Ok(temp_dir)
    }

    /// Executes the trial in a temporary directory
    /// Execute the trial in a temporary directory
    ///
    /// If `model` is provided, it will override the default model in the config.
    pub async fn execute(
        &self,
        sender: Option<mpsc::Sender<Event>>,
        model: Option<String>,
    ) -> Result<()> {
        let temp_dir = self.setup_temp_project()?;
        let mut conf = self.tenx_conf.clone();
        conf.session_store_dir = temp_dir.path().join("session");
        conf.project_root = ProjectRoot::Path(temp_dir.path().join(&self.trial_conf.project));
        if let Some(m) = model {
            conf.default_model = Some(m);
        }
        let tenx = Tenx::new(conf);

        let mut session = tenx.new_session_from_cwd(&sender)?;

        info!("trial setup complete");
        match &self.trial_conf.op {
            TrialOp::Ask(edit) => {
                for path in &edit.editable {
                    session.add_editable(&tenx.config, &path.to_string_lossy())?;
                }
                tenx.ask(&mut session, edit.prompt.clone(), sender).await?;
            }
            TrialOp::Fix(fix) => {
                for path in &fix.editable {
                    session.add_editable(&tenx.config, &path.to_string_lossy())?;
                }
                tenx.fix(&mut session, sender, fix.prompt.clone()).await?;
            }
        };
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

        let tenx_conf = match &trial_conf.config {
            Some(config) => config.clone(),
            None => Self::default_config()?,
        };

        Ok(Trial {
            name: name.to_string(),
            desc: trial_conf.desc.clone().unwrap_or_default(),
            base_dir: base_dir.as_ref().to_path_buf(),
            trial_conf,
            tenx_conf,
        })
    }
}

/// Lists all trials in a directory by finding .toml files and loading them as Trial objects.
pub fn list<P: AsRef<Path>>(base_dir: P) -> Result<Vec<Trial>> {
    let mut trials = Vec::new();
    let pattern = base_dir.as_ref().join("*.toml");
    let pattern = pattern.to_string_lossy();

    for entry in
        glob(&pattern).map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?
    {
        let path =
            entry.map_err(|e| TenxError::Internal(format!("Failed to read glob entry: {}", e)))?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| TenxError::Internal("Invalid trial file name".to_string()))?;

        if !name.ends_with(".conf") {
            if let Ok(trial) = Trial::load(&base_dir, name) {
                trials.push(trial);
            }
        }
    }

    Ok(trials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_trials() -> Result<()> {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir()?;
        let test_project_dir = dir.path().join("projects").join("test_project");
        fs::create_dir_all(&test_project_dir)?;

        let test_toml = r#"
            project = "test_project"
            [op.ask]
            prompt = "test prompt"
            editable = ["file1.rs"]
        "#;

        fs::write(dir.path().join("test1.toml"), test_toml)?;
        fs::write(dir.path().join("test2.toml"), test_toml)?;
        let trials = list(dir.path())?;
        assert_eq!(trials.len(), 2);
        assert!(trials.iter().any(|t| t.name == "test1"));
        assert!(trials.iter().any(|t| t.name == "test2"));

        Ok(())
    }

    #[test]
    fn test_trial_conf_from_str() -> Result<()> {
        let toml = r#"
            project = "test_project"
            desc = "Test trial description"
            [op.ask]
            prompt = "test prompt"
            editable = ["file1.rs", "file2.rs"]
            [config]
            anthropic_key = "test_key"
            no_preflight = true
        "#;

        let conf = TrialConf::from_str(toml)?;
        assert_eq!(conf.project, "test_project");

        match conf.op {
            TrialOp::Ask(edit) => {
                assert_eq!(edit.prompt, "test prompt");
                assert_eq!(
                    edit.editable,
                    vec![PathBuf::from("file1.rs"), PathBuf::from("file2.rs")]
                );
            }
            TrialOp::Fix(_) => panic!("Expected Ask variant"),
        }

        Ok(())
    }
}
