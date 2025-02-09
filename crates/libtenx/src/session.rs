//! A Session is the context and a sequence of model interaction steps.
use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{config, context, model::Usage, patch::Patch, state, strategy, Result, TenxError};

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

/// A single step in the session - basically a prompt and a patch.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Step {
    /// The name of the model used for this step
    pub model: String,
    /// The prompt provided to the model
    pub prompt: String,
    /// Time taken in seconds to receive the complete model response
    pub response_time: Option<f64>,
    /// An associated error, for instance an error processing a model response. This may be
    /// retryable, in which case a new step will be synthesized to go back to the model.
    pub err: Option<TenxError>,
    /// A cache of the file contents before the step was applied
    pub rollback_cache: HashMap<PathBuf, String>,

    /// The response from the model
    pub model_response: Option<ModelResponse>,
    pub rollback_id: u64,
}

impl Step {
    /// Creates a new Step with the given prompt and rollback ID.
    pub fn new(model: String, prompt: String, rollback_id: u64) -> Self {
        Step {
            model,
            prompt,
            rollback_id,
            model_response: None,
            response_time: None,
            err: None,
            rollback_cache: HashMap::new(),
        }
    }
}

/// A user-requested action, which may contain many steps.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Action {
    pub strategy: strategy::Strategy,
    /// The steps in the action
    steps: Vec<Step>,
}

impl Action {
    /// Creates a new Action with the given strategy.
    pub fn new(_config: &config::Config, strategy: strategy::Strategy) -> Result<Self> {
        Ok(Action {
            strategy,
            steps: Vec::new(),
        })
    }

    /// Returns a reference to the last step in the action
    pub fn last_step(&self) -> Option<&Step> {
        self.steps.last()
    }

    pub fn add_step(&mut self, step: Step) -> Result<()> {
        if let Some(last_step) = self.steps.last() {
            if last_step.model_response.is_none() && last_step.err.is_none() {
                return Err(TenxError::Internal(
                    "Cannot add a new prompt while the previous step has no response".into(),
                ));
            }
        }
        // step.rollback_id = self.state.mark()?;
        self.steps.push(step);
        Ok(())
    }
}

/// A serializable session, which persists between invocations.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Session {
    editable: Vec<PathBuf>,
    actions: Vec<Action>,
    pub state: state::State,
    pub contexts: Vec<context::Context>,
}

impl Session {
    /// Creates a new Session, configuring its state directory.
    ///
    /// If `dir` is provided, it is used as the project root; otherwise the configuration's
    /// project root is used.
    pub fn new(config: &config::Config) -> Result<Self> {
        Ok(Session {
            editable: vec![],
            actions: vec![],
            contexts: Vec::new(),
            state: config.state()?,
        })
    }

    /// Clears all contexts from the session.
    pub fn clear_ctx(&mut self) {
        self.contexts.clear();
    }

    /// Clears all actions in the session, but keeps the current editable and context intact.
    pub fn clear(&mut self) {
        self.actions.clear();
    }

    /// Returns all steps across all actions in the session.
    pub fn steps(&self) -> Vec<&Step> {
        self.actions
            .iter()
            .flat_map(|action| &action.steps)
            .collect()
    }

    /// Returns a reference to the last step in the session.
    pub fn last_step(&self) -> Option<&Step> {
        self.actions
            .iter()
            .rev()
            .flat_map(|action| action.steps.iter().rev())
            .next()
    }

    /// Returns a reference to the last action in the session.
    pub fn last_action(&self) -> Option<&Action> {
        self.actions.last()
    }

    /// Returns a reference to the last action in the session.
    pub fn last_action_mut(&mut self) -> Option<&mut Action> {
        self.actions.iter_mut().last()
    }

    /// Returns a mutable reference to the last step in the session.
    pub fn last_step_mut(&mut self) -> Option<&mut Step> {
        self.actions
            .iter_mut()
            .rev()
            .flat_map(|action| action.steps.iter_mut().rev())
            .next()
    }

    pub fn contexts(&self) -> &Vec<context::Context> {
        &self.contexts
    }

    /// Returns the relative paths of the editables for this session in sorted order.
    pub fn editables(&self) -> Vec<PathBuf> {
        let mut paths = self.editable.clone();
        paths.sort();
        paths
    }

    /// Does this session have a pending prompt?
    pub fn should_continue(&self) -> bool {
        if let Some(step) = self.steps().last() {
            step.model_response.is_none() && step.err.is_none()
        } else {
            false
        }
    }

    /// Adds a new step to the last action in the session.
    ///
    /// Returns an error if the last step doesn't have either a patch or an error.
    pub fn add_step(&mut self, model: String, prompt: String) -> Result<()> {
        if let Some(action) = self.actions.last_mut() {
            let rollback_id = self.state.mark()?;
            action.add_step(Step::new(model, prompt, rollback_id))?;
        } else {
            Err(TenxError::Internal("No actions in session".into()))?
        }
        Ok(())
    }

    /// Adds a new action to the session.
    pub fn add_action(&mut self, action: Action) -> Result<()> {
        self.actions.push(action);
        Ok(())
    }

    /// Adds a new context to the session.
    ///
    /// If a context with the same name and type already exists, it will be replaced.
    pub fn add_context(&mut self, new_context: context::Context) {
        if let Some(pos) = self.contexts.iter().position(|x| x.is_dupe(&new_context)) {
            self.contexts[pos] = new_context;
        } else {
            self.contexts.push(new_context);
        }
    }

    fn reset_steps(&mut self, _config: &config::Config, keep: Option<usize>) -> Result<()> {
        let total_steps: usize = self.actions.iter().map(|a| a.steps.len()).sum();
        match keep {
            Some(offset) if offset >= total_steps => {
                return Err(TenxError::Internal("Invalid rollback offset".into()));
            }
            Some(offset) => {
                let mut steps_remaining = offset + 1;
                let mut new_actions = Vec::new();

                // Preserve actions until we reach the required step count
                for action in &mut self.actions {
                    if steps_remaining == 0 {
                        break;
                    }

                    let keep_steps = std::cmp::min(action.steps.len(), steps_remaining);
                    // Keep first 'keep_steps' steps in this action
                    if keep_steps > 0 {
                        if keep_steps < action.steps.len() {
                            // Rollback steps being removed
                            for step in action.steps[keep_steps..].iter_mut().rev() {
                                self.state.revert(step.rollback_id)?;
                            }
                            action.steps.truncate(keep_steps);
                        }
                        new_actions.push(action.clone());
                    }

                    steps_remaining = steps_remaining.saturating_sub(action.steps.len());
                }

                self.actions = new_actions;
            }
            None => {
                self.state.revert(0)?;
                self.actions.clear();
            }
        }
        Ok(())
    }

    /// Resets the session to a specific step, removing and rolling back all subsequent steps.
    pub fn reset(&mut self, config: &config::Config, offset: usize) -> Result<()> {
        self.reset_steps(config, Some(offset))
    }

    /// Rolls back and removes all steps in the session.
    pub fn reset_all(&mut self, config: &config::Config) -> Result<()> {
        self.reset_steps(config, None)
    }

    /// Apply the last step in the session, applying the patch and operations.
    pub fn apply_last_step(&mut self, _config: &config::Config) -> Result<()> {
        let resp = self
            .last_step()
            .ok_or_else(|| TenxError::Internal("No steps in session".into()))?
            .model_response
            .clone()
            .ok_or_else(|| TenxError::Internal("No response in the last step".into()))?;
        if let Some(patch) = &resp.patch {
            self.state.patch(patch)?;
        }
        Ok(())
    }

    pub fn editables_for_step_state(&self, step_offset: usize) -> Result<Vec<PathBuf>> {
        // Convert step offset into a rollback ID range
        let total_steps: usize = self.actions.iter().map(|a| a.steps.len()).sum();
        if step_offset > total_steps {
            return Err(TenxError::Internal("Invalid step offset".into()));
        }

        let mut prev_rollback_id = None;
        let mut curr_rollback_id = None;
        let mut curr_offset = 0;

        // Find the rollback IDs for the target step
        for action in &self.actions {
            for step in &action.steps {
                if curr_offset == step_offset {
                    curr_rollback_id = Some(step.rollback_id);
                } else if curr_offset == step_offset.saturating_sub(1) {
                    prev_rollback_id = Some(step.rollback_id);
                }
                curr_offset += 1;
            }
        }
        self.state
            .last_changed_between(prev_rollback_id, curr_rollback_id)
    }

    /// Return the list of files that should be included in the editable block before a given step.
    pub fn editables_for_step(&self, step_offset: usize) -> Result<Vec<PathBuf>> {
        let total_steps: usize = self.actions.iter().map(|a| a.steps.len()).sum();
        if step_offset > total_steps {
            return Err(TenxError::Internal("Invalid step offset".into()));
        }

        let mut most_recent_modified: HashMap<PathBuf, i32> = self
            .editable
            .iter()
            .map(|path| (path.clone(), -1))
            .collect();

        let mut global_idx = 0;
        for action in &self.actions {
            for step in &action.steps {
                if let Some(resp) = &step.model_response {
                    if let Some(patch) = &resp.patch {
                        for path in patch.affected_files() {
                            most_recent_modified.insert(path.clone(), global_idx);
                        }
                    }
                    for op in &resp.operations {
                        let Operation::Edit(path) = op;
                        most_recent_modified.insert(path.clone(), global_idx);
                    }
                }
                global_idx += 1;
            }
        }

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

    #[test]
    fn test_add_context_ignores_duplicates() -> Result<()> {
        let mut test_project = crate::testutils::test_project();
        test_project.create_file_tree(&["test.txt"]);
        test_project.write("test.txt", "content");

        // Test Path context
        let path1 =
            context::Context::Path(context::Path::new(&test_project.config, "test.txt".into())?);
        let path2 =
            context::Context::Path(context::Path::new(&test_project.config, "test.txt".into())?);
        test_project.session.add_context(path1);
        test_project.session.add_context(path2);
        assert_eq!(test_project.session.contexts.len(), 1);

        // Test URL context
        let url1 = context::Context::Url(context::Url::new("http://example.com".into()));
        let url2 = context::Context::Url(context::Url::new("http://example.com".into()));
        test_project.session.add_context(url1);
        test_project.session.add_context(url2);
        assert_eq!(test_project.session.contexts.len(), 2);

        Ok(())
    }

    // #[test]
    // fn test_reset() -> Result<()> {
    //     let mut test_project = crate::testutils::test_project();
    //     test_project.create_file_tree(&["test.txt"]);
    //     test_project.write("test.txt", "Initial content");
    //
    //     test_project.session.add_action(Action::new(
    //         &test_project.config,
    //         strategy::Strategy::Code(strategy::Code::new("test".into())),
    //     )?)?;
    //
    //     // Add three steps
    //     for i in 1..=3 {
    //         let content = format!("Content {}", i);
    //         let patch = Patch {
    //             changes: vec![Change::Write(WriteFile {
    //                 path: PathBuf::from("test.txt"),
    //                 content: content.clone(),
    //             })],
    //         };
    //         test_project
    //             .session
    //             .add_step("test_model".into(), format!("Prompt {}", i))?;
    //
    //         if let Some(step) = test_project.session.last_step_mut() {
    //             step.model_response = Some(ModelResponse {
    //                 patch: Some(patch.clone()),
    //                 operations: vec![],
    //                 usage: None,
    //                 comment: Some(format!("Step {}", i)),
    //                 response_text: Some(format!("Step {}", i)),
    //             });
    //             step.apply(&test_project.config)?;
    //         }
    //     }
    //
    //     let steps = test_project.session.steps();
    //     assert_eq!(steps.len(), 3);
    //     assert_eq!(test_project.read("test.txt"), "Content 3");
    //
    //     // Rollback to the first step
    //     test_project.session.reset(&test_project.config, 0)?;
    //     let steps = test_project.session.steps();
    //     assert_eq!(steps.len(), 1);
    //     assert_eq!(test_project.read("test.txt"), "Content 1");
    //
    //     // Test reset_all
    //     test_project.session.reset_all(&test_project.config)?;
    //     let steps = test_project.session.steps();
    //     assert_eq!(steps.len(), 0);
    //     assert_eq!(test_project.read("test.txt"), "Initial content");
    //
    //     Ok(())
    // }
    //
    //
    // #[test]
    // fn test_editables_for_step() -> Result<()> {
    //     let mut test_project = crate::testutils::test_project();
    //     test_project.create_file_tree(&["file1.txt", "file2.txt", "file3.txt"]);
    //     test_project.write("file1.txt", "content1");
    //     test_project.write("file2.txt", "content2");
    //     test_project.write("file3.txt", "content3");
    //
    //     // Add all files as editable
    //     test_project
    //         .session
    //         .add_editable_path(&test_project.config, "file1.txt")?;
    //     test_project
    //         .session
    //         .add_editable_path(&test_project.config, "file2.txt")?;
    //     test_project
    //         .session
    //         .add_editable_path(&test_project.config, "file3.txt")?;
    //
    //     test_project.session.add_action(Action::new(
    //         &test_project.config,
    //         strategy::Strategy::Code(strategy::Code::new("test".into())),
    //     )?)?;
    //     // Test 1: Before any steps are added, all files should be marked as modified
    //     let editables = test_project.session.editables_for_step(0)?;
    //     assert_eq!(editables.len(), 3,);
    //
    //     // Step 0: Modify file1.txt through patch
    //     test_project
    //         .session
    //         .add_step("test_model".into(), "step0".into())?;
    //     let step = test_project.session.last_step_mut().unwrap();
    //     step.model_response = Some(ModelResponse {
    //         patch: Some(Patch {
    //             changes: vec![Change::Write(WriteFile {
    //                 path: PathBuf::from("file1.txt"),
    //                 content: "modified1".into(),
    //             })],
    //         }),
    //         operations: vec![],
    //         usage: None,
    //         comment: None,
    //         response_text: None,
    //     });
    //
    //     // Step 1: Request to edit file2.txt and modify file3.txt through patch
    //     test_project
    //         .session
    //         .add_step("test_model".into(), "step1".into())?;
    //     let step = test_project.session.last_step_mut().unwrap();
    //     step.model_response = Some(ModelResponse {
    //         patch: Some(Patch {
    //             changes: vec![Change::Write(WriteFile {
    //                 path: PathBuf::from("file3.txt"),
    //                 content: "modified3".into(),
    //             })],
    //         }),
    //         operations: vec![Operation::Edit(PathBuf::from("file2.txt"))],
    //         usage: None,
    //         comment: None,
    //         response_text: None,
    //     });
    //
    //     // Step 2: Empty step (no modifications)
    //     test_project
    //         .session
    //         .add_step("test_model".into(), "step2".into())?;
    //     let step = test_project.session.last_step_mut().unwrap();
    //     step.model_response = Some(ModelResponse {
    //         patch: None,
    //         operations: vec![],
    //         usage: None,
    //         comment: None,
    //         response_text: None,
    //     });
    //
    //     // Test 2: At step 0, no files should be editable (no previous step)
    //     let editables = test_project.session.editables_for_step(0)?;
    //     assert!(
    //         editables.is_empty(),
    //         "No files should be editable at step 0"
    //     );
    //
    //     // Test 3: At step 1, file1.txt should be editable (modified in step 0)
    //     let editables = test_project.session.editables_for_step(1)?;
    //     assert_eq!(editables.len(), 1, "One file should be editable");
    //     assert_eq!(editables[0], PathBuf::from("file1.txt"));
    //
    //     // Test 4: At step 2, both file2.txt and file3.txt should be editable (modified in step 1)
    //     let editables = test_project.session.editables_for_step(2)?;
    //     assert_eq!(editables.len(), 2, "Two files should be editable");
    //     assert!(editables.contains(&PathBuf::from("file2.txt")));
    //     assert!(editables.contains(&PathBuf::from("file3.txt")));
    //
    //     // Test 5: At step 3, no files should be editable (nothing modified in step 2)
    //     let editables = test_project.session.editables_for_step(3)?;
    //     assert!(
    //         editables.is_empty(),
    //         "No files should be editable at step 3"
    //     );
    //
    //     // Test 6: Error case - invalid step offset
    //     assert!(test_project.session.editables_for_step(5).is_err());
    //
    //     Ok(())
    // }

    // #[test]
    // fn test_apply_last_step_with_editable() -> Result<()> {
    //     let mut test_project = crate::testutils::test_project();
    //     test_project.create_file_tree(&["test.txt", "new.txt"]);
    //     test_project.write("test.txt", "content");
    //     test_project.write("new.txt", "new content");
    //
    //     test_project
    //         .session
    //         .add_action(Action::new(
    //             &test_project.config,
    //             strategy::Strategy::Code(strategy::Code::new("test".into())),
    //         )?)
    //         .unwrap();
    //
    //     // Add a step with both a patch and an edit operation
    //     test_project
    //         .session
    //         .add_step("test_model".into(), "test prompt".into())?;
    //     let step = test_project.session.last_step_mut().unwrap();
    //     let patch = Patch {
    //         changes: vec![
    //             Change::Write(WriteFile {
    //                 path: PathBuf::from("test.txt"),
    //                 content: "modified content".into(),
    //             }),
    //             Change::View(PathBuf::from("new.txt")),
    //         ],
    //     };
    //     step.model_response = Some(ModelResponse {
    //         patch: Some(patch),
    //         operations: vec![],
    //         usage: None,
    //         comment: None,
    //         response_text: None,
    //     });
    //     step.rollback_cache = [(PathBuf::from("test.txt"), "content".into())]
    //         .into_iter()
    //         .collect();
    //
    //     // Apply the last step
    //     test_project.session.apply_last_step(&test_project.config)?;
    //
    //     // Verify that both the patch was applied and the editable was added
    //     assert_eq!(test_project.read("test.txt"), "modified content");
    //     assert!(test_project
    //         .session
    //         .editable
    //         .contains(&PathBuf::from("new.txt")));
    //     assert_eq!(test_project.session.editable.len(), 1);
    //
    //     Ok(())
    // }
}
