use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    checks::check_paths,
    config::Config,
    error::Result,
    events::{send_event, Event, EventSender},
    session::{Session, Step},
};

use super::*;

/// Shared step data for Code and Fix strategies.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CodeStep {}

impl CodeStep {
    /// Creates a new CodeStep instance.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The Code strategy allows the model to write and modify code based on a prompt.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Code {}

impl Code {
    /// Creates a new Code strategy instance.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The Fix strategy is used to resolve errors in code by providing the model with error details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    error: String,
}

impl Fix {
    /// Creates a new Fix strategy with the specified error.
    pub fn new(error: &str) -> Self {
        Self {
            error: String::from(error),
        }
    }
}

/// Common logic for processing a step in both Code and Fix strategies.
///
/// This function:
/// 1. Checks for errors and patch failures in the current step
/// 2. Creates a new step with appropriate messages if needed
/// 3. Returns the current state of the action
fn process_step(
    config: &Config,
    session: &mut Session,
    _action_offset: usize,
    step: &Step,
    events: Option<EventSender>,
) -> Result<ActionState> {
    let model = config.models.default.clone();
    let mut messages = Vec::new();
    let mut user_message = Vec::new();

    // Check for retryable errors
    if let Some(err) = &step.err {
        if let Some(err_message) = err.should_retry() {
            messages.push(err_message.to_string());
            user_message.push(format!("{}", err));
        }
    }

    // Check for patch application failures
    if let Some(patch_info) = &step.patch_info {
        if !patch_info.failures.is_empty() {
            let failure_messages = patch_info
                .failures
                .iter()
                .map(|(change, err)| format!("Failed to apply change {:?}: {}", change, err))
                .collect::<Vec<_>>()
                .join("\n\n");

            messages.push(format!(
                "Please fix the following issues with your changes:\n\n{}",
                failure_messages
            ));
            user_message.push("patch failures".into());
        }
    }

    // If we have errors or patch failures, create a new step
    if !messages.is_empty() {
        let model_message = messages.join("\n\n");
        let user = user_message.join(", ");

        send_event(
            &events,
            Event::NextStep {
                user,
                model: model_message.clone(),
            },
        )?;
        debug!("Next step, based on errors and/or patch failures");

        let new_step = Step::new(model, model_message, StrategyStep::Code(CodeStep::new()));
        session.last_action_mut()?.add_step(new_step)?;

        return Ok(ActionState {
            completion: Completion::Incomplete,
            input_required: InputRequired::No,
        });
    }

    // Check for operations in model response that need further action
    if let Some(model_response) = &step.model_response {
        if !model_response.operations.is_empty() {
            let model_message = "OK".to_string();
            send_event(
                &events,
                Event::NextStep {
                    user: "operations applied".into(),
                    model: model_message.clone(),
                },
            )?;
            debug!("Next step, based on operations");

            let new_step = Step::new(model, model_message, StrategyStep::Code(CodeStep::new()));
            session.last_action_mut()?.add_step(new_step)?;

            return Ok(ActionState {
                completion: Completion::Incomplete,
                input_required: InputRequired::No,
            });
        }
    }

    // No issues found, action is complete
    Ok(ActionState {
        completion: Completion::Complete,
        input_required: InputRequired::No,
    })
}

/// Determines the current state of an action
fn get_action_state(action: &crate::session::Action) -> ActionState {
    if action.steps().is_empty() {
        return ActionState {
            completion: Completion::Incomplete,
            input_required: InputRequired::Yes,
        };
    }

    if let Some(step) = action.last_step() {
        if step.is_incomplete() || step.should_continue() {
            return ActionState {
                completion: Completion::Incomplete,
                input_required: InputRequired::No,
            };
        } else {
            return ActionState {
                completion: Completion::Complete,
                input_required: InputRequired::No,
            };
        }
    }

    // Fallback (shouldn't happen)
    ActionState {
        completion: Completion::Incomplete,
        input_required: InputRequired::No,
    }
}

/// Renders a step with common rendering logic for both Code and Fix strategies
fn render_step<R: crate::render::Render>(
    step: &Step,
    renderer: &mut R,
    step_header: &str,
    show_success: bool,
) -> Result<()> {
    renderer.push(step_header);

    // Add prompt
    renderer.push("prompt");
    renderer.para(&step.raw_prompt);
    renderer.pop();

    // Add comment from model response if present
    if let Some(model_response) = &step.model_response {
        if let Some(comment) = &model_response.comment {
            renderer.push("model comment");
            renderer.para(comment);
            renderer.pop();
        }
    }

    // Add error if present
    if let Some(err) = &step.err {
        renderer.push("error");
        renderer.para(&err.to_string());
        renderer.pop();
    }

    // Add patch information if present
    if let Some(patch_info) = &step.patch_info {
        if !patch_info.failures.is_empty() {
            let failure_messages: Vec<String> = patch_info
                .failures
                .iter()
                .map(|(change, err)| format!("Failed to apply {:?}: {}", change, err))
                .collect();

            renderer.push("Patch failures:");
            renderer.bullets(failure_messages);
            renderer.pop();
        } else if show_success && patch_info.succeeded > 0 {
            renderer.para(&format!(
                "Successfully applied {} changes",
                patch_info.succeeded
            ));
        }
    }

    renderer.pop();
    Ok(())
}

impl ActionStrategy for Code {
    fn name(&self) -> &'static str {
        "code"
    }

    fn check(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        events: Option<EventSender>,
    ) -> Result<()> {
        let paths = session.get_action(action_offset)?.state.touched()?;
        check_paths(config, &paths, &events)
    }

    fn next_step(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        events: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<ActionState> {
        let action = session.get_action(action_offset)?;

        if let Some(step) = action.last_step() {
            // If the last step is incomplete, don't synthesize a new step
            if step.is_incomplete() {
                return Ok(ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::No,
                });
            }

            // Clone to avoid borrow issues when calling process_step
            let step_clone = step.clone();
            process_step(config, session, action_offset, &step_clone, events)
        } else {
            // First step in the action
            if let Some(p) = prompt {
                let model = config.models.default.clone();
                let new_step = Step::new(model, p, StrategyStep::Code(CodeStep::new()));
                session.last_action_mut()?.add_step(new_step)?;

                Ok(ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::No,
                })
            } else {
                // Need user input for first step
                Ok(ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::Yes,
                })
            }
        }
    }

    fn state(&self, _config: &Config, session: &Session, action_offset: usize) -> ActionState {
        match session.actions.get(action_offset) {
            Some(action) => get_action_state(action),
            None => ActionState {
                completion: Completion::Incomplete,
                input_required: InputRequired::No,
            },
        }
    }

    fn render<R: crate::render::Render>(
        &self,
        _config: &Config,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
        renderer: &mut R,
    ) -> Result<()> {
        let step = session.get_action(action_offset)?.steps()[step_offset].clone();
        let header = format!("step {}:{}", action_offset, step_offset);
        render_step(&step, renderer, &header, false)
    }
}

impl ActionStrategy for Fix {
    fn name(&self) -> &'static str {
        "fix"
    }

    fn check(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        events: Option<EventSender>,
    ) -> Result<()> {
        let paths = session.get_action(action_offset)?.state.touched()?;
        check_paths(config, &paths, &events)
    }

    fn next_step(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        events: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<ActionState> {
        let action = session.get_action(action_offset)?;

        if let Some(step) = action.last_step() {
            // If the last step is incomplete, don't synthesize a new step
            if step.is_incomplete() {
                return Ok(ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::No,
                });
            }

            // Clone to avoid borrow issues when calling process_step
            let step_clone = step.clone();
            process_step(config, session, action_offset, &step_clone, events)
        } else {
            // First step in the action
            let model = config.models.default.clone();
            let default_prompt = format! {"Please fix the following errors: {}\n", self.error};
            let new_step = Step::new(
                model,
                prompt.unwrap_or(default_prompt),
                StrategyStep::Code(CodeStep::new()),
            );
            session.last_action_mut()?.add_step(new_step)?;

            Ok(ActionState {
                completion: Completion::Incomplete,
                input_required: InputRequired::No,
            })
        }
    }

    fn state(&self, _config: &Config, session: &Session, action_offset: usize) -> ActionState {
        match session.actions.get(action_offset) {
            Some(action) => {
                if action.steps().is_empty() {
                    return ActionState {
                        completion: Completion::Incomplete,
                        input_required: InputRequired::Optional,
                    };
                }

                get_action_state(action)
            }
            None => ActionState {
                completion: Completion::Incomplete,
                input_required: InputRequired::No,
            },
        }
    }

    fn render<R: crate::render::Render>(
        &self,
        _config: &Config,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
        renderer: &mut R,
    ) -> Result<()> {
        let step = session.get_action(action_offset)?.steps()[step_offset].clone();

        // Create the header
        let header = format!("Step {}", step_offset);

        // If it's the first step, show the error we're fixing before rendering the common parts
        if step_offset == 0 {
            renderer.push(&header);
            renderer.para(&format!("Fixing error: {}", self.error));
            renderer.pop();
        }

        render_step(&step, renderer, &header, true)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::error::TenxError;
    use crate::session::Action;
    use crate::strategy::Strategy;
    use crate::testutils::test_project;

    #[test]
    fn test_code_next_step() -> Result<()> {
        let test_project = test_project();
        let code = Code::new();
        let mut session = Session::new(&test_project.config)?;

        // Empty session should return an error now
        let result = code.next_step(&test_project.config, &mut session, 0, None, None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TenxError::Internal(_)));

        session.add_action(Action::new(
            &test_project.config,
            Strategy::Code(code.clone()),
        )?)?;
        let action_idx = session.actions.len() - 1;

        // Without prompt should request user input
        let state = code.next_step(&test_project.config, &mut session, action_idx, None, None)?;
        assert_eq!(state.input_required, InputRequired::Yes);

        // With prompt should add a step
        let mut session_clone = session.clone();
        let state = code.next_step(
            &test_project.config,
            &mut session_clone,
            action_idx,
            None,
            Some("Test".into()),
        )?;
        assert_eq!(state.input_required, InputRequired::No);
        assert_eq!(session_clone.last_step().unwrap().raw_prompt, "Test");

        // Test retry with patch error
        session.last_action_mut()?.add_step(Step::new(
            test_project.config.models.default.clone(),
            "Test".into(),
            StrategyStep::Code(CodeStep::new()),
        ))?;
        let patch_err = TenxError::Patch {
            user: "Error".into(),
            model: "Retry".into(),
        };
        session.last_step_mut().unwrap().err = Some(patch_err);

        let state = code.next_step(&test_project.config, &mut session, action_idx, None, None)?;
        assert_eq!(state.completion, Completion::Incomplete);
        assert_eq!(session.last_step().unwrap().raw_prompt, "Retry");

        // Non-retryable error should complete the action
        let mut session_clone = session.clone();
        session_clone.last_step_mut().unwrap().err = Some(TenxError::Config("Error".into()));

        let state = code.next_step(
            &test_project.config,
            &mut session_clone,
            action_idx,
            None,
            None,
        )?;

        assert_eq!(state.completion, Completion::Complete);

        Ok(())
    }

    #[test]
    fn test_fix_next_step() -> Result<()> {
        let test_project = test_project();
        let mut session = Session::new(&test_project.config)?;

        // Empty session should return an error now
        let fix = Fix::new("error");
        let result = fix.next_step(&test_project.config, &mut session, 0, None, None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TenxError::Internal(_)));

        // Add an action and test custom prompt
        session.add_action(Action::new(
            &test_project.config,
            Strategy::Fix(fix.clone()),
        )?)?;
        let action_idx = session.actions.len() - 1;

        let state = fix.next_step(
            &test_project.config,
            &mut session,
            action_idx,
            None,
            Some("Fix prompt".into()),
        )?;

        assert_eq!(state.completion, Completion::Incomplete);
        assert_eq!(session.last_step().unwrap().raw_prompt, "Fix prompt");

        // Test retryable error
        session.last_step_mut().unwrap().err = Some(TenxError::Patch {
            user: "Error".into(),
            model: "Retry".into(),
        });

        let state = fix.next_step(&test_project.config, &mut session, action_idx, None, None)?;
        assert_eq!(state.completion, Completion::Incomplete);
        assert_eq!(session.last_step().unwrap().raw_prompt, "Retry");

        // Test default prompt in a new action
        let fix2 = Fix::new("error");
        let mut session2 = Session::new(&test_project.config)?;
        session2.add_action(Action::new(
            &test_project.config,
            Strategy::Fix(fix2.clone()),
        )?)?;
        let action_idx2 = session2.actions.len() - 1;

        let state = fix2.next_step(&test_project.config, &mut session2, action_idx2, None, None)?;
        assert_eq!(state.completion, Completion::Incomplete);
        assert!(session2
            .last_step()
            .unwrap()
            .raw_prompt
            .starts_with("Please fix the following errors"),);

        Ok(())
    }
}
