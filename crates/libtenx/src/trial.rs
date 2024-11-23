//! Trial module for defining and running test trials.
//!
//! A trial consists of a TrialConf at NAME.ron which specifies the operations to perform, as well
//! as an embedded tenx configuration.
use std::{
    fs,
    path::{Path, PathBuf},
};

use fs_extra;
use glob::glob;
use serde::Deserialize;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tracing::info;

use crate::{
    config::{Config, Include, ProjectRoot},
    model::ModelProvider,
    Event, Result, Tenx, TenxError,
};

#[derive(Debug, Clone, Deserialize)]
pub enum TrialOp {
    Code {
        prompt: String,
        editable: Vec<PathBuf>,
    },
    Fix {
        prompt: Option<String>,
        editable: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrialConf {
    pub project: String,
    pub op: TrialOp,
    pub config: Option<Config>,
    pub desc: String,
}

impl TrialConf {
    /// Parse a RON string into a TrialConf
    fn from_str(s: &str) -> Result<Self> {
        let options = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        options
            .from_str(s)
            .map_err(|e| TenxError::Internal(format!("Failed to parse trial RON: {}", e)))
    }

    /// Read a trial configuration from a RON file
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

#[derive(Debug)]
pub struct Trial {
    pub name: String,
    pub desc: String,
    pub base_dir: PathBuf,
    pub trial_conf: TrialConf,
    pub tenx_conf: Config,
}

/// A report about a trial execution.
#[derive(Debug)]
pub struct TrialReport {
    /// Name of the trial
    pub trial_name: String,
    /// Name of the model used
    pub model_name: String,
    /// Whether the trial failed
    pub failed: bool,
    /// Number of steps taken
    pub steps: usize,
    /// Total tokens used in input
    pub tokens_in: u64,
    /// Total tokens used in output
    pub tokens_out: u64,
    /// Number of patch errors
    pub error_patch: usize,
    /// Number of validation errors
    pub error_validation: usize,
    /// Number of response parse errors
    pub error_response_parse: usize,
    /// Number of other errors
    pub error_other: usize,
    /// Total execution time in seconds
    pub time_taken: f64,
}

impl TrialReport {
    /// Computes a trial report from a session
    pub fn from_session(
        session: &crate::Session,
        trial_name: String,
        model_name: String,
        time_taken: f64,
    ) -> Self {
        let steps = session.steps().len();
        let (tokens_in, tokens_out) = session
            .steps()
            .iter()
            .filter_map(|step| step.model_response.as_ref()?.usage.as_ref())
            .map(|usage| usage.totals())
            .fold((0, 0), |(acc_in, acc_out), (in_, out)| {
                (acc_in + in_, acc_out + out)
            });
        let failed = session.last_step_error().is_some();

        let mut error_patch = 0;
        let mut error_validation = 0;
        let mut error_response_parse = 0;
        let mut error_other = 0;

        for step in session.steps() {
            if let Some(err) = &step.err {
                match err {
                    TenxError::Patch { .. } => error_patch += 1,
                    TenxError::Check { .. } => error_validation += 1,
                    TenxError::ResponseParse { .. } => error_response_parse += 1,
                    _ => error_other += 1,
                }
            }
        }

        TrialReport {
            trial_name,
            model_name,
            failed,
            steps,
            tokens_in,
            tokens_out,
            error_patch,
            error_validation,
            error_response_parse,
            error_other,
            time_taken,
        }
    }
}

impl Trial {
    /// Creates a temporary directory and copies the project into it. The project will be placed at
    /// "$tempdir/project" regardless of source directory name.
    fn setup_temp_project(&self) -> Result<TempDir> {
        let temp_dir = TempDir::new()
            .map_err(|e| TenxError::Internal(format!("Failed to create temp directory: {}", e)))?;

        let mut src_path = self.base_dir.clone();
        src_path.push("projects");
        src_path.push(&self.trial_conf.project);

        let dst_path = temp_dir.path().to_path_buf();

        fs_extra::dir::copy(&src_path, &dst_path, &fs_extra::dir::CopyOptions::new())
            .map_err(|e| TenxError::Internal(format!("Failed to copy project directory: {}", e)))?;

        // For a project path like "foo/bar", we want to rename "bar" to "project"
        let path_buf = PathBuf::from(&self.trial_conf.project);
        let project_name = path_buf
            .components()
            .last()
            .and_then(|c| c.as_os_str().to_str())
            .ok_or_else(|| TenxError::Internal("Invalid project name".to_string()))?;

        let copied_dir = dst_path.join(project_name);
        fs::rename(&copied_dir, dst_path.join("project")).map_err(|e| {
            TenxError::Internal(format!("Failed to rename project directory: {}", e))
        })?;

        fs::create_dir_all(dst_path.join("session")).map_err(|e| {
            TenxError::Internal(format!("Failed to create session directory: {}", e))
        })?;

        Ok(temp_dir)
    }

    /// Execute the trial in a temporary directory
    ///
    /// If `model` is provided, it will override the default model in the config.
    pub async fn execute(
        &self,
        sender: Option<mpsc::Sender<Event>>,
        model: Option<String>,
    ) -> Result<(TrialReport, crate::Session)> {
        use std::time::Instant;
        let start_time = Instant::now();
        let temp_dir = self.setup_temp_project()?;
        let mut conf = self.tenx_conf.clone();
        conf.session_store_dir = PathBuf::from("");
        conf.project_root = ProjectRoot::Path(temp_dir.path().join("project"));
        if let Some(m) = model {
            conf.default_model = Some(m);
        }
        let model_name = conf.model()?.name();
        let tenx = Tenx::new(conf);

        let mut session = tenx.new_session_from_cwd(&sender).await?;

        info!("trial setup complete: {}", self.name);
        let result = match &self.trial_conf.op {
            TrialOp::Code { prompt, editable } => {
                for path in editable {
                    session.add_editable(&tenx.config, &path.to_string_lossy())?;
                }
                tenx.code(&mut session, prompt.clone(), sender).await
            }
            TrialOp::Fix { prompt, editable } => {
                for path in editable {
                    session.add_editable(&tenx.config, &path.to_string_lossy())?;
                }
                tenx.fix(&mut session, sender, prompt.clone()).await
            }
        };

        match &result {
            Ok(_) => info!("trial completed successfully: {}", self.name),
            Err(e) => info!("trial failed: {}: {}", self.name, e),
        }

        let time_taken = start_time.elapsed().as_secs_f64();
        Ok((
            TrialReport::from_session(&session, self.name.clone(), model_name, time_taken),
            session,
        ))
    }

    /// Returns a default configuration for trials. These need to be over-ridden by the trial
    /// if needed.
    fn default_config() -> Result<Config> {
        let mut config = Config::default();
        config.include = Include::Glob(vec!["**/*".to_string()]);
        config.exclude = vec!["target/**".to_string()];
        config.retry_limit = 1;
        // We disable streaming for trials by default, because streaming messes up token counts in
        // complicated ways.
        config.no_stream = true;
        Ok(config)
    }

    /// Loads a trial from the base directory with the specified name
    pub fn load<P: AsRef<Path>>(base_dir: P, name: &str) -> Result<Self> {
        info!("loading trial: {}", name);
        let mut path = PathBuf::from(base_dir.as_ref());
        path.push(format!("{}.ron", name));
        let trial_conf = TrialConf::read(&path)?;
        trial_conf.validate(&base_dir)?;

        let mut tenx_conf = Self::default_config()?;
        if let Some(config) = &trial_conf.config {
            tenx_conf.merge(config);
        }

        Ok(Trial {
            name: name.to_string(),
            desc: trial_conf.desc.clone(),
            base_dir: base_dir.as_ref().to_path_buf(),
            trial_conf,
            tenx_conf,
        })
    }
}

/// Returns trials that match any of the provided patterns, without duplicates.
pub fn list<P: AsRef<Path>>(base_dir: P, patterns: Option<&[&str]>) -> Result<Vec<Trial>> {
    let mut trials = Vec::new();
    let fs_pattern = base_dir.as_ref().join("*.ron");
    let fs_pattern = fs_pattern.to_string_lossy();

    for entry in glob(&fs_pattern)
        .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?
    {
        let path =
            entry.map_err(|e| TenxError::Internal(format!("Failed to read glob entry: {}", e)))?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| TenxError::Internal("Invalid trial file name".to_string()))?;
        if let Ok(trial) = Trial::load(&base_dir, name) {
            trials.push(trial);
        }
    }

    if let Some(patterns) = patterns {
        let compiled_patterns: Vec<glob::Pattern> = patterns
            .iter()
            .map(|p| {
                glob::Pattern::new(p)
                    .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(trials
            .into_iter()
            .filter(|t| compiled_patterns.iter().any(|p| p.matches(&t.name)))
            .collect())
    } else {
        Ok(trials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_trials_with_glob() -> Result<()> {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir()?;
        let test_project_dir = dir.path().join("projects").join("test_project");
        fs::create_dir_all(&test_project_dir)?;

        let test_ron = r#"(
            project: "test_project",
            desc: "Test trial description",
            op: Code(
                prompt: "test prompt",
                editable: ["file1.rs"],
            )
        )"#;

        fs::write(dir.path().join("test1.ron"), test_ron)?;
        fs::write(dir.path().join("test2.ron"), test_ron)?;
        fs::write(dir.path().join("other.ron"), test_ron)?;

        // Test with no patterns
        let trials = list(dir.path(), None)?;
        assert_eq!(trials.len(), 3);
        assert!(trials.iter().any(|t| t.name == "test1"));
        assert!(trials.iter().any(|t| t.name == "test2"));
        assert!(trials.iter().any(|t| t.name == "other"));

        // Test with single pattern
        let trials = list(dir.path(), Some(&["test1"]))?;
        assert_eq!(trials.len(), 1);
        assert_eq!(trials[0].name, "test1");

        // Test with wildcard pattern
        let trials = list(dir.path(), Some(&["test*"]))?;
        assert_eq!(trials.len(), 2);
        assert!(trials.iter().any(|t| t.name == "test1"));
        assert!(trials.iter().any(|t| t.name == "test2"));

        // Test with multiple patterns
        let trials = list(dir.path(), Some(&["test1", "other"]))?;
        assert_eq!(trials.len(), 2);
        assert!(trials.iter().any(|t| t.name == "test1"));
        assert!(trials.iter().any(|t| t.name == "other"));

        // Test with overlapping patterns
        let trials = list(dir.path(), Some(&["test*", "test1"]))?;
        assert_eq!(trials.len(), 2);
        assert!(trials.iter().any(|t| t.name == "test1"));
        assert!(trials.iter().any(|t| t.name == "test2"));

        Ok(())
    }

    #[test]
    fn test_setup_temp_project_nested() -> Result<()> {
        use std::fs;
        use tempfile::tempdir;

        // Create a temporary directory to act as our base directory
        let base_dir = tempdir()?;

        // Create a nested test project structure
        let test_project = base_dir
            .path()
            .join("projects")
            .join("nested")
            .join("test_proj");
        fs::create_dir_all(&test_project)?;
        fs::write(test_project.join("test.txt"), "test content")?;

        // Create a trial configuration with nested project path
        let trial = Trial {
            name: "test".to_string(),
            desc: "test description".to_string(),
            base_dir: base_dir.path().to_path_buf(),
            trial_conf: TrialConf {
                project: "nested/test_proj".to_string(),
                op: TrialOp::Code {
                    prompt: "test".to_string(),
                    editable: vec![],
                },
                config: None,
                desc: "Test trial".to_string(),
            },
            tenx_conf: Config::default(),
        };

        // Run setup_temp_project
        let temp_dir = trial.setup_temp_project()?;

        // Verify the directory structure
        let project_dir = temp_dir.path().join("project");
        assert!(project_dir.exists());
        assert!(project_dir.is_dir());

        // Verify the content was copied
        let test_file = project_dir.join("test.txt");
        assert!(test_file.exists());
        assert_eq!(fs::read_to_string(test_file)?, "test content");

        Ok(())
    }

    #[test]
    fn test_setup_temp_project() -> Result<()> {
        use std::fs;
        use tempfile::tempdir;

        // Create a temporary directory to act as our base directory
        let base_dir = tempdir()?;

        // Create a test project structure
        let test_project = base_dir.path().join("projects").join("test_proj");
        fs::create_dir_all(&test_project)?;
        fs::write(test_project.join("test.txt"), "test content")?;

        // Create a trial configuration
        let trial = Trial {
            name: "test".to_string(),
            desc: "test description".to_string(),
            base_dir: base_dir.path().to_path_buf(),
            trial_conf: TrialConf {
                project: "test_proj".to_string(),
                op: TrialOp::Code {
                    prompt: "test".to_string(),
                    editable: vec![],
                },
                config: None,
                desc: "Test trial".to_string(),
            },
            tenx_conf: Config::default(),
        };

        // Run setup_temp_project
        let temp_dir = trial.setup_temp_project()?;

        // Verify the directory structure
        let project_dir = temp_dir.path().join("project");
        let session_dir = temp_dir.path().join("session");

        assert!(project_dir.exists());
        assert!(project_dir.is_dir());
        assert!(session_dir.exists());
        assert!(session_dir.is_dir());

        // Verify the content was copied
        let test_file = project_dir.join("test.txt");
        assert!(test_file.exists());
        assert_eq!(fs::read_to_string(test_file)?, "test content");

        Ok(())
    }

    #[test]
    fn test_trial_conf_from_str() -> Result<()> {
        let ron = r#"(
            project: "test_project",
            desc: "Test trial description",
            op: Code(
                prompt: "test prompt",
                editable: ["file1.rs", "file2.rs"],
            ),
            config: (
                no_pre: true,
            )
        )"#;

        let conf = TrialConf::from_str(ron)?;
        assert_eq!(conf.project, "test_project");

        match conf.op {
            TrialOp::Code { prompt, editable } => {
                assert_eq!(prompt, "test prompt");
                assert_eq!(
                    editable,
                    vec![PathBuf::from("file1.rs"), PathBuf::from("file2.rs")]
                );
            }
            TrialOp::Fix { .. } => panic!("Expected Code variant"),
        }

        Ok(())
    }
}
