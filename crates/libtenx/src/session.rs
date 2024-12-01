use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use fs_err as fs;
use serde::{Deserialize, Serialize};

use crate::{config, context, model::Usage, patch::Patch, Result, TenxError};

/// A parsed model response
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
pub struct ModelResponse {
    /// Model's comment on changes
    pub comment: Option<String>,
    /// The unified patch in the response
    pub patch: Option<Patch>,
    /// Operations requested by the model, other than patching.
    pub operations: Vec<Operation>,
    /// Model-specific usage statistics
    pub usage: Option<Usage>,
    /// The verbatim text response from the model
    pub response_text: Option<String>,
}

/// Operations requested by the model, other than patching.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Request to edit a file
    Edit(PathBuf),
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub enum StepType {
    /// A user code request
    Code,

    /// A fix request
    Fix,

    /// An automatically generated step. This might be needed if, for instance, the model asks to
    /// edit a file, in order to mantain request/response sequencing.
    Auto,

    /// A prompt generated to handle a retryable error
    Error,
}

/// A single step in the session - basically a prompt and a patch.
#[derive(Debug, Deserialize, Serialize)]
pub struct Step {
    /// The name of the model used for this step
    pub model: String,
    /// The type of step
    pub step_type: StepType,
    /// The prompt provided to the model
    pub prompt: String,
    /// The response from the model
    pub model_response: Option<ModelResponse>,
    /// Time taken in seconds to receive the complete model response
    pub response_time: Option<f64>,
    /// An associated error, for instance an error processing a model response. This may be
    /// retryable, in which case a new step will be synthesized to go back to the model.
    pub err: Option<TenxError>,
    /// A cache of the file contents before the step was applied
    pub rollback_cache: HashMap<PathBuf, String>,
}

impl Step {
    /// Creates a new Step with the given prompt.
    pub fn new(model: String, prompt: String, step_type: StepType) -> Self {
        Step {
            model,
            step_type,
            prompt,
            model_response: None,
            response_time: None,
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
    pub steps: Vec<Step>,
    pub editable: Vec<PathBuf>,
    pub contexts: Vec<context::Context>,
}

impl Session {
    /// Clears all contexts from the session.
    pub fn clear_ctx(&mut self) {
        self.contexts.clear();
    }

    /// Updates the prompt at a specific step.
    pub fn update_prompt_at(&mut self, offset: usize, prompt: String) -> Result<()> {
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
    pub fn should_continue(&self) -> bool {
        if let Some(step) = self.steps.last() {
            step.model_response.is_none() && step.err.is_none()
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
    pub fn add_prompt(&mut self, model: String, prompt: String, step_type: StepType) -> Result<()> {
        if let Some(last_step) = self.steps.last() {
            if last_step.model_response.is_none() && last_step.err.is_none() {
                return Err(TenxError::Internal(
                    "Cannot add a new prompt while the previous step has no response".into(),
                ));
            }
        }
        self.steps.push(Step::new(model, prompt, step_type));
        Ok(())
    }

    /// Sets the prompt for the last step in the session.
    /// If there are no steps, it creates a new one.
    pub fn set_last_prompt(
        &mut self,
        model: String,
        prompt: String,
        step_type: StepType,
    ) -> Result<()> {
        if self.steps.is_empty() {
            self.steps.push(Step::new(model, prompt, step_type));
            Ok(())
        } else if let Some(last_step) = self.steps.last_mut() {
            last_step.prompt = prompt;
            last_step.model = model;
            last_step.step_type = step_type;
            last_step.model_response = None;
            Ok(())
        } else {
            Err(TenxError::Internal("Failed to set prompt".into()))
        }
    }

    /// Adds a new context to the session.
    ///
    /// If a context with the same name and type already exists, it will be replaced.
    pub fn add_context(&mut self, new_context: context::Context) {
        if let Some(pos) = self.contexts.iter().position(|x| x == &new_context) {
            self.contexts[pos] = new_context;
        } else {
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
        let mut had_edit = false;
        for operation in &resp.operations {
            match operation {
                Operation::Edit(path) => {
                    self.add_editable_path(config, path)?;
                    had_edit = true;
                }
            }
        }
        if had_edit {
            // Use the same model as the current step for auto-prompts
            let current_model = self
                .steps
                .last()
                .map(|s| s.model.clone())
                .unwrap_or_default();
            self.add_prompt(current_model, "OK".into(), StepType::Auto)?;
        }
        Ok(())
    }

    /// Return the list of files that should be included in the editable block before a given step.
    /// - Files are included for step N, if they are modified in step N-1, AND that modification
    ///   is the last modification in to the file in the step list.
    /// - Edit operations and appearing in a Patch are both counted as a modifications.
    /// - Offset can be num_steps + 1, meaning we're considering all steps and calculating the edit
    ///   set for a new step to be added.
    /// - Passing an offset beyond num_steps + 1 is an error
    pub fn editables_for_step(&self, step_offset: usize) -> Result<Vec<PathBuf>> {
        if step_offset > self.steps.len() {
            return Err(TenxError::Internal("Invalid step offset".into()));
        }

        // Initialize with all files having -1 as their last modification step
        let mut most_recent_modified: HashMap<PathBuf, i32> = self
            .editable
            .iter()
            .map(|path| (path.clone(), -1))
            .collect();

        // Record all file modifications with their step index
        for (idx, step) in self.steps.iter().enumerate() {
            if let Some(resp) = &step.model_response {
                // Add files modified by patches
                if let Some(patch) = &resp.patch {
                    for path in patch.changed_files() {
                        most_recent_modified.insert(path.clone(), idx as i32);
                    }
                }
                // Add files that were requested for editing
                for op in &resp.operations {
                    let Operation::Edit(path) = op;
                    most_recent_modified.insert(path.clone(), idx as i32);
                }
            }
        }

        // Return files modified in the previous step
        let target_step = (step_offset as i32) - 1;
        Ok(self
            .editable
            .iter()
            .filter(|path| most_recent_modified.get(*path) == Some(&target_step))
            .cloned()
            .collect())
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

        test_project.config.project.include = config::Include::Glob(vec!["**/*".to_string()]);

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
            };
            test_project.session.add_prompt(
                "test_model".into(),
                format!("Prompt {}", i),
                StepType::Code,
            )?;

            let rollback_cache = [(PathBuf::from("test.txt"), test_project.read("test.txt"))]
                .into_iter()
                .collect();

            if let Some(step) = test_project.session.last_step_mut() {
                step.model_response = Some(ModelResponse {
                    patch: Some(patch.clone()),
                    operations: vec![],
                    usage: None,
                    comment: Some(format!("Step {}", i)),
                    response_text: Some(format!("Step {}", i)),
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
    fn test_editables_for_step() -> Result<()> {
        let mut test_project = crate::testutils::test_project();
        test_project.create_file_tree(&["file1.txt", "file2.txt", "file3.txt"]);
        test_project.write("file1.txt", "content1");
        test_project.write("file2.txt", "content2");
        test_project.write("file3.txt", "content3");

        // Add all files as editable
        test_project
            .session
            .add_editable_path(&test_project.config, "file1.txt")?;
        test_project
            .session
            .add_editable_path(&test_project.config, "file2.txt")?;
        test_project
            .session
            .add_editable_path(&test_project.config, "file3.txt")?;

        // Test 1: Before any steps are added, all files should be marked as modified
        let editables = test_project.session.editables_for_step(0)?;
        assert_eq!(editables.len(), 3,);

        // Step 0: Modify file1.txt through patch
        test_project
            .session
            .add_prompt("test_model".into(), "step0".into(), StepType::Code)?;
        let step = test_project.session.steps.last_mut().unwrap();
        step.model_response = Some(ModelResponse {
            patch: Some(Patch {
                changes: vec![Change::Write(WriteFile {
                    path: PathBuf::from("file1.txt"),
                    content: "modified1".into(),
                })],
            }),
            operations: vec![],
            usage: None,
            comment: None,
            response_text: None,
        });

        // Step 1: Request to edit file2.txt and modify file3.txt through patch
        test_project
            .session
            .add_prompt("test_model".into(), "step1".into(), StepType::Code)?;
        let step = test_project.session.steps.last_mut().unwrap();
        step.model_response = Some(ModelResponse {
            patch: Some(Patch {
                changes: vec![Change::Write(WriteFile {
                    path: PathBuf::from("file3.txt"),
                    content: "modified3".into(),
                })],
            }),
            operations: vec![Operation::Edit(PathBuf::from("file2.txt"))],
            usage: None,
            comment: None,
            response_text: None,
        });

        // Step 2: Empty step (no modifications)
        test_project
            .session
            .add_prompt("test_model".into(), "step2".into(), StepType::Code)?;
        let step = test_project.session.steps.last_mut().unwrap();
        step.model_response = Some(ModelResponse {
            patch: None,
            operations: vec![],
            usage: None,
            comment: None,
            response_text: None,
        });

        // Test 2: At step 0, no files should be editable (no previous step)
        let editables = test_project.session.editables_for_step(0)?;
        assert!(
            editables.is_empty(),
            "No files should be editable at step 0"
        );

        // Test 3: At step 1, file1.txt should be editable (modified in step 0)
        let editables = test_project.session.editables_for_step(1)?;
        assert_eq!(editables.len(), 1, "One file should be editable");
        assert_eq!(editables[0], PathBuf::from("file1.txt"));

        // Test 4: At step 2, both file2.txt and file3.txt should be editable (modified in step 1)
        let editables = test_project.session.editables_for_step(2)?;
        assert_eq!(editables.len(), 2, "Two files should be editable");
        assert!(editables.contains(&PathBuf::from("file2.txt")));
        assert!(editables.contains(&PathBuf::from("file3.txt")));

        // Test 5: At step 3, no files should be editable (nothing modified in step 2)
        let editables = test_project.session.editables_for_step(3)?;
        assert!(
            editables.is_empty(),
            "No files should be editable at step 3"
        );

        // Test 6: Error case - invalid step offset
        assert!(test_project.session.editables_for_step(5).is_err());

        Ok(())
    }

    #[test]
    fn test_apply_last_step_with_editable() -> Result<()> {
        let mut test_project = crate::testutils::test_project();
        test_project.create_file_tree(&["test.txt", "new.txt"]);
        test_project.write("test.txt", "content");
        test_project.write("new.txt", "new content");

        // Add a step with both a patch and an edit operation
        test_project.session.add_prompt(
            "test_model".into(),
            "test prompt".into(),
            StepType::Code,
        )?;
        let step = test_project.session.steps.last_mut().unwrap();
        let patch = Patch {
            changes: vec![Change::Write(WriteFile {
                path: PathBuf::from("test.txt"),
                content: "modified content".into(),
            })],
        };
        step.model_response = Some(ModelResponse {
            patch: Some(patch),
            operations: vec![Operation::Edit(PathBuf::from("new.txt"))],
            usage: None,
            comment: None,
            response_text: None,
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
