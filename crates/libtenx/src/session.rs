//! l Session is the context and a sequence of model interaction steps.
use std::path::PathBuf;

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

    /// The raw text response from the model
    pub raw_response: Option<String>,
}

/// Operations requested by the model, other than patching.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub enum Operation {}

/// A single step in the session - basically a prompt and a patch.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Step {
    /// The name of the model used for this step
    pub model: String,

    /// The raw prompt provided to the model
    pub raw_prompt: String,

    /// Time in seconds to receive the complete model response
    pub response_time: Option<f64>,

    /// An associated error, for instance an error processing a model response. This may be
    /// retryable, in which case a new step will be synthesized to go back to the model.
    pub err: Option<TenxError>,

    /// Information about the patch applied in this step, including any failures.
    pub patch_info: Option<state::PatchInfo>,

    /// The response from the model
    pub model_response: Option<ModelResponse>,

    /// The rollback identifier for this step. Rolling back to this identifier will revert all
    /// changes.
    pub rollback_id: u64,
}

impl Step {
    /// Creates a new Step with the given prompt and rollback ID.
    pub fn new(model: String, prompt: String) -> Self {
        Step {
            model,
            raw_prompt: prompt,
            rollback_id: 0,
            model_response: None,
            response_time: None,
            patch_info: None,
            err: None,
        }
    }

    /// Is this step incomplete?
    pub fn is_incomplete(&self) -> bool {
        self.model_response.is_none() && self.err.is_none()
    }

    /// Returns true if a step should continue, based on:
    /// a) there is a patch error, or
    /// b) there is a step error, and the error's should_retry() is not None.
    pub fn should_continue(&self) -> bool {
        if self
            .patch_info
            .as_ref()
            .is_some_and(|p| !p.failures.is_empty())
        {
            return true;
        }

        if let Some(err) = &self.err {
            if err.should_retry().is_some() {
                return true;
            }
        }

        false
    }
}

/// A user-requested action, which may contain many steps.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Action {
    pub strategy: strategy::Strategy,
    pub state: state::State,
    /// The steps in the action
    steps: Vec<Step>,
}

impl Action {
    /// Creates a new Action with the given strategy.
    pub fn new(config: &config::Config, strategy: strategy::Strategy) -> Result<Self> {
        Ok(Action {
            strategy,
            steps: Vec::new(),
            state: config.state()?,
        })
    }

    /// Returns a reference to the last step in the action
    pub fn last_step(&self) -> Option<&Step> {
        self.steps.last()
    }

    /// Returns all steps in the action.
    pub fn steps(&self) -> &Vec<Step> {
        &self.steps
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
    pub contexts: Vec<context::Context>,
}

impl Session {
    /// Creates a new Session, configuring its state directory.
    ///
    /// If `dir` is provided, it is used as the project root; otherwise the configuration's
    /// project root is used.
    pub fn new(_config: &config::Config) -> Result<Self> {
        Ok(Session {
            editable: vec![],
            actions: vec![],
            contexts: Vec::new(),
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

    /// Returns all actions in the session.
    pub fn actions(&self) -> &Vec<Action> {
        &self.actions
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
    pub fn last_action(&self) -> Result<&Action> {
        self.actions
            .last()
            .ok_or_else(|| TenxError::Internal("No actions in session".into()))
    }

    /// Returns a reference to the last action in the session.
    pub fn last_action_mut(&mut self) -> Result<&mut Action> {
        self.actions
            .iter_mut()
            .last()
            .ok_or_else(|| TenxError::Internal("No actions in session".into()))
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
    pub fn add_step(&mut self, mut step: Step) -> Result<()> {
        if let Some(action) = self.actions.last_mut() {
            let rollback_id = action.state.mark()?;
            step.rollback_id = rollback_id;
            action.add_step(step)?;
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

    /// Reset the session to a specific action and step, removing all subsequent steps.
    ///
    /// * `action_idx` - The 0-based index of the action to keep steps for
    /// * `step_idx` - The 0-based index of the step within the action to keep
    pub fn reset(&mut self, action_idx: usize, step_idx: Option<usize>) -> Result<()> {
        if action_idx >= self.actions.len() {
            return Err(TenxError::Internal(format!(
                "Invalid action index: {}",
                action_idx
            )));
        }

        // Validate step index if provided
        if let Some(step_idx) = step_idx {
            let action = &self.actions[action_idx];
            if step_idx >= action.steps.len() {
                return Err(TenxError::Internal(format!(
                    "Invalid step index {} for action {}, which has {} steps",
                    step_idx,
                    action_idx,
                    action.steps.len()
                )));
            }
        }

        // Revert state changes after the target step
        let action = &mut self.actions[action_idx];
        if let Some(next_step_idx) = step_idx.map(|i| i + 1) {
            if next_step_idx < action.steps.len() {
                action
                    .state
                    .revert(action.steps[next_step_idx].rollback_id)?;
            }
        }

        // Truncate steps in the current action
        if let Some(step_idx) = step_idx {
            action.steps.truncate(step_idx + 1);
        }

        // Remove all actions after the target action
        self.actions.truncate(action_idx + 1);

        Ok(())
    }

    /// Rolls back and removes all steps in the session.
    pub fn reset_all(&mut self) -> Result<()> {
        if let Some(action) = self.actions.first_mut() {
            action.state.revert(0)?;
        }
        self.actions.clear();
        Ok(())
    }

    /// Apply the last step in the session, applying the patch and operations. The step must
    /// already have a model response.
    pub fn apply_last_step(&mut self, _config: &config::Config) -> Result<()> {
        let resp = self
            .last_step()
            .ok_or_else(|| TenxError::Internal("No steps in session".into()))?
            .model_response
            .clone()
            .ok_or_else(|| TenxError::Internal("No response in the last step".into()))?;
        if let Some(patch) = &resp.patch {
            let patch_info = self.actions.last_mut().unwrap().state.patch(patch)?;
            let step = self
                .last_step_mut()
                .ok_or_else(|| TenxError::Internal("No steps in session".into()))?;
            step.patch_info = Some(patch_info);
        }
        Ok(())
    }

    /// Get editables for a specific action and step in the session
    pub fn editables_for_step_state(
        &self,
        action_idx: usize,
        step_idx: usize,
    ) -> Result<Vec<PathBuf>> {
        if action_idx >= self.actions.len() {
            return Err(TenxError::Internal(format!(
                "Invalid action index: {}",
                action_idx
            )));
        }

        let action = &self.actions[action_idx];
        if step_idx >= action.steps.len() {
            return Err(TenxError::Internal(format!(
                "Invalid step index {} for action {}, which has {} steps",
                step_idx,
                action_idx,
                action.steps.len()
            )));
        }

        let curr_rollback_id = Some(action.steps[step_idx].rollback_id);

        // Get the previous rollback id (if this isn't the first step)
        let prev_rollback_id = if step_idx > 0 {
            Some(action.steps[step_idx - 1].rollback_id)
        } else if action_idx > 0 {
            // Get the last step of the previous action
            let prev_action = &self.actions[action_idx - 1];
            prev_action.steps.last().map(|s| s.rollback_id)
        } else {
            None
        };

        action
            .state
            .last_changed_between(prev_rollback_id, curr_rollback_id)
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
}
