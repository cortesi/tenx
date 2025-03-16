//! Session is the context and a sequence of model interaction steps.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    config, context,
    error::{Result, TenxError},
    model::Usage,
    state::{self, patch::Patch},
    strategy::{self, ActionStrategy, StrategyStep},
};
use unirend::Detail;

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

/// A single step in the session - single prompt and model response. Steps also store
/// processed information from the active strategy in `strategy_step`.
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

    /// The response from the model
    pub model_response: Option<ModelResponse>,

    /// Information about the patch applied in this step, including any failures.
    pub patch_info: Option<state::PatchInfo>,

    /// The rollback identifier for this step. Rolling back to this identifier will revert all
    /// changes.
    pub rollback_id: u64,
    pub strategy_step: StrategyStep,
}

impl Step {
    /// Creates a new Step with the given prompt and rollback ID.
    pub fn new(model: String, raw_prompt: String, strategy_step: StrategyStep) -> Self {
        Step {
            model,
            raw_prompt,
            rollback_id: 0,
            model_response: None,
            response_time: None,
            patch_info: None,
            err: None,
            strategy_step,
        }
    }

    /// Reset the step, clearing all response data and setting the rollback ID. The rollback ID is
    /// required because presumably the state has been rolled back before this call.
    pub fn reset(&mut self, rollback_id: u64) {
        self.model_response = None;
        self.response_time = None;
        self.patch_info = None;
        self.err = None;
        self.rollback_id = rollback_id;
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

    /// Adds a new step to the action.
    ///
    /// Returns an error if the last step doesn't have either a model response or an error.
    pub fn add_step(&mut self, mut step: Step) -> Result<()> {
        if let Some(last_step) = self.steps.last() {
            if last_step.model_response.is_none() && last_step.err.is_none() {
                return Err(TenxError::Internal(
                    "Cannot add a new prompt while the previous step has no response".into(),
                ));
            }
        }
        let rollback_id = self.state.mark()?;
        step.rollback_id = rollback_id;
        self.steps.push(step);
        Ok(())
    }

    /// Render the action using the provided renderer
    pub fn render<R: unirend::Render>(
        &self,
        config: &config::Config,
        session: &Session,
        action_offset: usize,
        renderer: &mut R,
        detail: Detail,
    ) -> Result<()> {
        renderer.push(&format!("{}: {}", action_offset, self.strategy.name()));

        // Add list of touched files if there are any
        if let Ok(touched_files) = self.state.touched() {
            if !touched_files.is_empty() {
                renderer.push("files");
                let file_strings: Vec<String> = touched_files
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect();
                renderer.bullets(file_strings);
                renderer.pop();
            }
        }

        for (step_offset, _) in self.steps.iter().enumerate() {
            self.strategy.render(
                config,
                session,
                action_offset,
                step_offset,
                renderer,
                detail,
            )?;
        }
        renderer.pop();
        Ok(())
    }
}

/// A serializable session, which persists between invocations.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Session {
    pub actions: Vec<Action>,
    pub contexts: context::ContextManager,
}

impl Session {
    /// Creates a new Session, configuring its state directory.
    ///
    /// If `dir` is provided, it is used as the project root; otherwise the configuration's
    /// project root is used.
    pub fn new(_config: &config::Config) -> Result<Self> {
        Ok(Session {
            actions: vec![],
            contexts: context::ContextManager::new(),
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

    /// Get action and validate it exists
    pub fn get_action(&self, action_offset: usize) -> Result<&crate::session::Action> {
        self.actions
            .get(action_offset)
            .ok_or_else(|| TenxError::Internal(format!("Invalid action offset: {}", action_offset)))
    }

    pub fn get_step(
        &self,
        action_offset: usize,
        step_offset: usize,
    ) -> Result<&crate::session::Step> {
        let action = self.get_action(action_offset)?;
        action
            .steps
            .get(step_offset)
            .ok_or_else(|| TenxError::Internal(format!("Invalid step offset: {}", step_offset)))
    }

    /// Returns a reference to the last action in the session.
    pub fn last_action(&self) -> Result<&Action> {
        self.actions
            .last()
            .ok_or_else(|| TenxError::Internal("No actions in session".into()))
    }

    /// Returns a reference to the last step in the session.
    pub fn last_step(&self) -> Option<&Step> {
        self.last_action()
            .ok()
            .and_then(|action| action.last_step())
    }

    /// Returns a mutable reference to the last step in the session.
    pub fn last_step_mut(&mut self) -> Option<&mut Step> {
        self.last_action_mut()
            .ok()
            .and_then(|action| action.steps.last_mut())
    }

    /// Does this session have a pending prompt?
    pub fn should_continue(&self) -> bool {
        if let Some(step) = self.last_step() {
            step.model_response.is_none() && step.err.is_none()
        } else {
            false
        }
    }

    /// Returns a mutable reference to the last action in the session or an error if there are no actions.
    pub fn last_action_mut(&mut self) -> Result<&mut Action> {
        self.actions
            .iter_mut()
            .last()
            .ok_or_else(|| TenxError::Internal("No actions in session".into()))
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
        self.contexts.add(new_context);
    }

    /// Reset the session to a specific action and step, removing all subsequent steps.
    ///
    /// * `action_idx` - The 0-based index of the action to keep steps for
    /// * `step_idx` - The 0-based index of the step within the action to keep.
    ///   When None, keeps all steps in the action.
    pub fn reset(&mut self, action_idx: usize, step_idx: Option<usize>) -> Result<()> {
        if action_idx >= self.actions.len() {
            return Err(TenxError::Internal(format!(
                "Invalid action index: {}",
                action_idx
            )));
        }

        // Get a reference to the target action
        let action = &mut self.actions[action_idx];

        // If step_idx is provided, handle step-specific operations
        if let Some(step_idx) = step_idx {
            // Validate step index
            if step_idx >= action.steps.len() {
                return Err(TenxError::Internal(format!(
                    "Invalid step index {} for action {}, which has {} steps",
                    step_idx,
                    action_idx,
                    action.steps.len()
                )));
            }

            // Revert state changes after the target step
            let next_step_idx = step_idx + 1;
            if next_step_idx < action.steps.len() {
                action
                    .state
                    .revert(action.steps[next_step_idx].rollback_id)?;
            }

            // Truncate steps in the current action
            action.steps.truncate(step_idx + 1);
        }

        // Remove all actions after the target action
        self.actions.truncate(action_idx + 1);

        Ok(())
    }

    /// Reset the session to a specific action and step and prepare it for retry.
    /// This method first resets the session to the specified step, then clears the step's
    /// response data, and reverts to the step's rollback_id to reset the state.
    ///
    /// * `action_idx` - The 0-based index of the action containing the step to retry
    /// * `step_idx` - The 0-based index of the step within the action to retry
    pub fn retry(&mut self, action_idx: usize, step_idx: usize) -> Result<()> {
        // First reset the session to the specified action and step
        self.reset(action_idx, Some(step_idx))?;

        // Get a mutable reference to the step and reset it
        if action_idx < self.actions.len() {
            let action = &mut self.actions[action_idx];
            if step_idx < action.steps.len() {
                let step = &mut action.steps[step_idx];

                // Reset the step's response data with a new rollback ID
                step.reset(action.state.mark()?);
            }
        }

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

    /// Render the session using the provided renderer
    pub fn render<R: unirend::Render>(
        &self,
        config: &config::Config,
        renderer: &mut R,
        detail: Detail,
    ) -> Result<()> {
        renderer.push("session");
        if !self.contexts.is_empty() {
            renderer.push("context");
            self.contexts.render(renderer, detail)?;
            renderer.pop();
        }
        for (action_offset, action) in self.actions.iter().enumerate() {
            action.render(config, self, action_offset, renderer, detail)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::code;
    use crate::strategy::Strategy;
    use crate::testutils;

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

    #[test]
    fn test_retry_resets_step() -> Result<()> {
        let tp = testutils::test_project();
        // Use a Code strategy from the code module.
        let strategy = Strategy::Code(code::Code::new());
        let mut action = Action::new(&tp.config, strategy)?;

        // Add the first step.
        let mut step1 = Step::new(
            "model1".into(),
            "prompt1".into(),
            strategy::StrategyStep::Code(strategy::CodeStep::default()),
        );
        step1.model_response = Some(ModelResponse {
            comment: Some("first response".into()),
            patch: None,
            operations: vec![],
            usage: None,
            raw_response: Some("first raw".into()),
        });
        action.add_step(step1)?;

        // Add the second step.
        let mut step2 = Step::new(
            "model1".into(),
            "prompt2".into(),
            strategy::StrategyStep::Code(strategy::CodeStep::default()),
        );
        step2.model_response = Some(ModelResponse {
            comment: Some("second response".into()),
            patch: None,
            operations: vec![],
            usage: None,
            raw_response: Some("second raw".into()),
        });
        action.add_step(step2)?;

        // Create a session containing this action.
        let mut session = Session {
            actions: vec![action],
            contexts: context::ContextManager::new(),
        };

        // Call retry on the second step (index 1) of the first action.
        session.retry(0, 1)?;

        // After retry, the targeted step should have its response data cleared.
        let action = session.actions.first().unwrap();
        assert_eq!(action.steps.len(), 2);
        let retried_step = action.steps.get(1).unwrap();
        assert!(retried_step.model_response.is_none());
        assert!(retried_step.response_time.is_none());
        assert!(retried_step.patch_info.is_none());
        assert!(retried_step.err.is_none());

        Ok(())
    }
}
