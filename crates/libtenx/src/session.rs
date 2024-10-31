use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use fs_err as fs;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    config, context, events::Event, model::ModelProvider, model::Usage, patch::Patch,
    prompt::Prompt, Result, TenxError,
};

/// A parsed model response
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModelResponse {
    /// The unified patch in the response
    pub patch: Option<Patch>,
    /// Operations requested by the model, other than patching.
    pub operations: Vec<Operation>,
    /// Model-specific usage statistics
    pub usage: Option<Usage>,
}

/// Operations requested by the model, other than patching.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum Operation {
    /// Request to edit a file
    Edit(PathBuf),
}

/// A single step in the session - basically a prompt and a patch.
#[derive(Debug, Deserialize, Serialize)]
pub struct Step {
    /// The prompt provided to the model
    pub prompt: Prompt,
    /// The response from the model
    pub model_response: Option<ModelResponse>,
    /// An error from the model
    pub err: Option<TenxError>,
    /// A cache of the file contents before the step was applied
    pub rollback_cache: HashMap<PathBuf, String>,
}

impl Step {
    /// Creates a new Step with the given prompt.
    pub fn new(prompt: Prompt) -> Self {
        Step {
            prompt,
            model_response: None,
            err: None,
            rollback_cache: HashMap::new(),
        }
    }

    /// Applies the changes in this step, first caching the original file contents.
    fn apply(&mut self, config: &config::Config) -> Result<()> {
        if let Some(resp) = &self.model_response {
            if let Some(patch) = &resp.patch {
                self.rollback_cache = patch.snapshot(config)?;
                patch.apply(config)?;
            }
        }
        Ok(())
    }

    /// Rolls back any changes made in this step.
    pub fn rollback(&mut self, config: &config::Config) -> Result<()> {
        for (path, content) in &self.rollback_cache {
            fs::write(config.abspath(path)?, content)?;
        }
        self.model_response = None;
        self.err = None;
        Ok(())
    }
}

/// Determines if a given string is a glob pattern or a rooted path.
///
/// Returns false if the string starts with "./", indicating a rooted path.
/// Returns true otherwise, suggesting it might be a glob pattern.
fn is_glob(s: &str) -> bool {
    !s.starts_with("./")
}

/// A serializable session, which persists between invocations.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Session {
    steps: Vec<Step>,
    pub(crate) contexts: Vec<context::Context>,
    editable: Vec<PathBuf>,
}

impl Session {
    /// Updates the prompt at a specific step.
    pub fn update_prompt_at(&mut self, offset: usize, prompt: Prompt) -> Result<()> {
        if offset >= self.steps.len() {
            return Err(TenxError::Internal("Invalid step offset".into()));
        }
        self.steps[offset].prompt = prompt;
        Ok(())
    }

    /// Clears all steps in the session, but keeps the current editable and context intact.
    pub fn clear(&mut self) {
        self.steps.clear();
    }
}

impl Session {
    pub fn steps(&self) -> &Vec<Step> {
        &self.steps
    }

    pub fn steps_mut(&mut self) -> &mut Vec<Step> {
        &mut self.steps
    }

    /// Returns a reference to the last step in the session.
    pub fn last_step(&self) -> Option<&Step> {
        self.steps.last()
    }

    /// Returns a mutable reference to the last step in the session.
    pub fn last_step_mut(&mut self) -> Option<&mut Step> {
        self.steps.last_mut()
    }

    pub fn contexts(&self) -> &Vec<context::Context> {
        &self.contexts
    }

    /// Returns the relative paths of the editables for this session.
    pub fn editable(&self) -> &Vec<PathBuf> {
        &self.editable
    }

    /// Returns the absolute paths of the editables for this session.
    pub fn abs_editables(&self, config: &config::Config) -> Result<Vec<PathBuf>> {
        self.editable
            .clone()
            .iter()
            .map(|p| config.abspath(p))
            .collect()
    }

    /// Does this session have a pending prompt?
    pub fn pending_prompt(&self) -> bool {
        if let Some(step) = self.steps.last() {
            step.model_response.is_none()
        } else {
            false
        }
    }

    /// Return the error if the last step has one, else None.
    pub fn last_step_error(&self) -> Option<&TenxError> {
        self.last_step().and_then(|step| step.err.as_ref())
    }

    /// Adds a new step to the session, and sets the step prompt.
    ///
    /// Returns an error if the last step doesn't have either a patch or an error.
    pub fn add_prompt(&mut self, prompt: Prompt) -> Result<()> {
        if let Some(last_step) = self.steps.last() {
            if last_step.model_response.is_none() && last_step.err.is_none() {
                return Err(TenxError::Internal(
                    "Cannot add a new prompt while the previous step has no response".into(),
                ));
            }
        }
        self.steps.push(Step::new(prompt));
        Ok(())
    }

    /// Sets the prompt for the last step in the session.
    /// If there are no steps, it creates a new one.
    pub fn set_last_prompt(&mut self, prompt: Prompt) -> Result<()> {
        if self.steps.is_empty() {
            self.steps.push(Step::new(prompt));
            Ok(())
        } else if let Some(last_step) = self.steps.last_mut() {
            last_step.model_response = None;
            Ok(())
        } else {
            Err(TenxError::Internal("Failed to set prompt".into()))
        }
    }

    /// Adds a new context to the session, ignoring duplicates.
    ///
    /// If a context with the same name and type already exists, it will not be added again.
    pub fn add_context(&mut self, new_context: context::Context) {
        if !self.contexts.contains(&new_context) {
            self.contexts.push(new_context);
        }
    }

    /// Adds an editable file path to the session, normalizing relative paths.
    pub fn add_editable_path<P: AsRef<Path>>(
        &mut self,
        config: &config::Config,
        path: P,
    ) -> Result<usize> {
        let normalized_path = config.normalize_path(path)?;
        if !self.editable.contains(&normalized_path) {
            self.editable.push(normalized_path);
            Ok(1)
        } else {
            Ok(0)
        }
    }

    /// Adds editable files to the session based on a glob pattern.
    pub fn add_editable_glob(&mut self, config: &config::Config, pattern: &str) -> Result<usize> {
        let matched_files = config.match_files_with_glob(pattern)?;
        let mut added = 0;
        for file in matched_files {
            added += self.add_editable_path(config, file)?;
        }
        Ok(added)
    }

    /// Adds context to the session, either as a single file or as a glob pattern.
    /// Adds an editable file or glob pattern to the session.
    pub fn add_editable(&mut self, config: &config::Config, path: &str) -> Result<usize> {
        if is_glob(path) {
            self.add_editable_glob(config, path)
        } else {
            self.add_editable_path(config, path)
        }
    }

    /// Resets the session to a specific step, removing and rolling back all subsequent steps.
    pub fn reset(&mut self, config: &config::Config, offset: usize) -> Result<()> {
        if offset >= self.steps.len() {
            return Err(TenxError::Internal("Invalid rollback offset".into()));
        }

        let n = self.steps.len() - offset - 1;
        for step in self.steps.iter_mut().rev().take(n) {
            step.rollback(config)?;
        }

        self.steps.truncate(offset + 1);
        Ok(())
    }

    /// Prompts the current model with the session's state and sets the resulting patch and usage.
    pub async fn prompt(
        &mut self,
        config: &config::Config,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let mut model = config.model()?;
        let (patch, usage) = model.send(config, self, sender).await?;
        if let Some(last_step) = self.last_step_mut() {
            last_step.model_response = Some(ModelResponse {
                patch: Some(patch),
                operations: vec![],
                usage: Some(usage),
            });
        }
        Ok(())
    }

    /// Apply the last step in the session, applying the patch and operations.
    pub fn apply_last_step(&mut self, config: &config::Config) -> Result<()> {
        let step = self
            .last_step_mut()
            .ok_or_else(|| TenxError::Internal("No steps in session".into()))?;
        let resp = step
            .model_response
            .clone()
            .ok_or_else(|| TenxError::Internal("No response in the last step".into()))?;

        step.apply(config)?;
        for operation in &resp.operations {
            match operation {
                Operation::Edit(path) => {
                    self.add_editable_path(config, path)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config,
        context::ContextProvider,
        patch::{Change, Patch, WriteFile},
    };

    #[test]
    fn test_add_context_ignores_duplicates() {
        let mut test_project = crate::testutils::test_project();
        test_project.create_file_tree(&["test.txt"]);
        test_project.write("test.txt", "content");

        test_project.config.include = config::Include::Glob(vec!["**/*".to_string()]);

        let context1 = context::Context::Path(
            context::Path::new(&test_project.config, "test.txt".to_string()).unwrap(),
        );
        let context2 = context::Context::Path(
            context::Path::new(&test_project.config, "test.txt".to_string()).unwrap(),
        );

        test_project.session.add_context(context1.clone());
        test_project.session.add_context(context2);

        assert_eq!(test_project.session.contexts.len(), 1);
        assert!(matches!(
            test_project.session.contexts[0],
            context::Context::Path(_)
        ));

        if let context::Context::Path(glob_context) = &test_project.session.contexts[0] {
            let context_items = glob_context
                .contexts(&test_project.config, &test_project.session)
                .unwrap();
            assert_eq!(context_items[0].body, "content");
        } else {
            panic!("Expected Glob context");
        }
    }

    #[test]
    fn test_reset() -> Result<()> {
        let mut test_project = crate::testutils::test_project();
        test_project.create_file_tree(&["test.txt"]);
        test_project.write("test.txt", "Initial content");

        // Add three steps
        for i in 1..=3 {
            let content = format!("Content {}", i);
            let patch = Patch {
                changes: vec![Change::Write(WriteFile {
                    path: PathBuf::from("test.txt"),
                    content: content.clone(),
                })],
                comment: Some(format!("Step {}", i)),
            };
            test_project
                .session
                .add_prompt(Prompt::User(format!("Prompt {}", i)))?;

            let rollback_cache = [(PathBuf::from("test.txt"), test_project.read("test.txt"))]
                .into_iter()
                .collect();

            if let Some(step) = test_project.session.last_step_mut() {
                step.model_response = Some(ModelResponse {
                    patch: Some(patch.clone()),
                    operations: vec![],
                    usage: None,
                });
                step.rollback_cache = rollback_cache;
                step.apply(&test_project.config)?;
            }
        }

        assert_eq!(test_project.session.steps.len(), 3);
        assert_eq!(test_project.read("test.txt"), "Content 3");

        // Rollback to the first step
        test_project.session.reset(&test_project.config, 0)?;

        assert_eq!(test_project.session.steps.len(), 1);
        assert_eq!(test_project.read("test.txt"), "Content 1");

        Ok(())
    }

    #[test]
    fn test_apply_last_step_with_editable() -> Result<()> {
        let mut test_project = crate::testutils::test_project();
        test_project.create_file_tree(&["test.txt", "new.txt"]);
        test_project.write("test.txt", "content");
        test_project.write("new.txt", "new content");

        // Add a step with both a patch and an edit operation
        test_project
            .session
            .add_prompt(Prompt::User("test prompt".into()))?;
        let step = test_project.session.steps.last_mut().unwrap();
        let patch = Patch {
            changes: vec![Change::Write(WriteFile {
                path: PathBuf::from("test.txt"),
                content: "modified content".into(),
            })],
            comment: None,
        };
        step.model_response = Some(ModelResponse {
            patch: Some(patch),
            operations: vec![Operation::Edit(PathBuf::from("new.txt"))],
            usage: None,
        });
        step.rollback_cache = [(PathBuf::from("test.txt"), "content".into())]
            .into_iter()
            .collect();

        // Apply the last step
        test_project.session.apply_last_step(&test_project.config)?;

        // Verify that both the patch was applied and the editable was added
        assert_eq!(test_project.read("test.txt"), "modified content");
        assert!(test_project
            .session
            .editable
            .contains(&PathBuf::from("new.txt")));
        assert_eq!(test_project.session.editable.len(), 1);

        Ok(())
    }
}
