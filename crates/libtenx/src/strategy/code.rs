use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    checks::check_paths,
    config::Config,
    error::Result,
    error::TenxError,
    events::{send_event, Event, EventSender},
    session::{Action, Step},
};
use unirend::{Detail, Render, Style};

use super::*;

/// Shared step data for Code and Fix strategies.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CodeStep {
    pub user_input: Option<String>,
}

impl CodeStep {
    /// Creates a new CodeStep instance.
    pub fn new(user_input: Option<String>) -> Self {
        Self { user_input }
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
    // If any messages are pushed onto here for the model, the step is incomplete.
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
        if patch_info.should_continue {
            messages.push("Operations applied".to_string());
            user_message.push("operations applied".into());
        }
    }

    // Check for operations in model response that need further action
    if let Some(model_response) = &step.model_response {
        if !model_response.operations.is_empty() {
            let model_message = "Operations applied".to_string();
            messages.push(model_message.clone());
            send_event(
                &events,
                Event::NextStep {
                    user: "operations applied".into(),
                    model: model_message.clone(),
                },
            )?;
        }
    }

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
        let new_step = Step::new(
            model,
            model_message,
            StrategyStep::Code(CodeStep::default()),
        );
        session.last_action_mut()?.add_step(new_step)?;

        debug!("Action incomplete: creating next step");
        Ok(ActionState {
            completion: Completion::Incomplete,
            input_required: InputRequired::No,
        })
    } else {
        debug!("Action complete");
        Ok(ActionState {
            completion: Completion::Complete,
            input_required: InputRequired::No,
        })
    }
}

/// Determines the current state of an action
fn get_action_state(action: &Action) -> ActionState {
    if action.steps.is_empty() {
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
fn render_step<R: Render>(
    step: &Step,
    renderer: &mut R,
    step_header: &str,
    show_success: bool,
    detail: Detail,
) -> Result<()> {
    renderer.push(step_header);
    #[allow(unreachable_patterns)]
    let astep = match &step.strategy_step {
        StrategyStep::Code(astep) => astep,
        _ => return Err(TenxError::Internal("Invalid strategy step".into())),
    };

    if detail == Detail::Full {
        renderer.push("raw prompt");
        renderer.para(&step.raw_prompt);
        renderer.pop();
        if let Some(model_response) = &step.model_response {
            if let Some(raw_response) = &model_response.raw_response {
                renderer.push("raw response");
                renderer.para(raw_response);
                renderer.pop();
            }
        }
    } else {
        if let Some(user_input) = &astep.user_input {
            renderer.push("prompt");
            renderer.para(user_input);
            renderer.pop();
        }
        if let Some(model_response) = &step.model_response {
            if let Some(comment) = &model_response.comment {
                renderer.push("model comment");
                renderer.para(comment);
                renderer.pop();
            }
        }
    }

    if let Some(model_response) = &step.model_response {
        if let Some(patch) = &model_response.patch {
            renderer.push("patch");
            patch.render(renderer, detail)?;
            renderer.pop();
        }
    }

    // Add error if present
    if let Some(err) = &step.err {
        if err.should_retry().is_some() {
            renderer.push_style("retryable error", Style::Warn);
        } else {
            renderer.push_style("fatal error", Style::Error);
        }
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
                let raw_prompt = p.clone();
                let new_step = Step::new(
                    model,
                    raw_prompt,
                    StrategyStep::Code(CodeStep::new(Some(p))),
                );
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

    fn render<R: unirend::Render>(
        &self,
        _config: &Config,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
        renderer: &mut R,
        detail: Detail,
    ) -> Result<()> {
        let step = session.get_action(action_offset)?.steps[step_offset].clone();
        let header = format!("step {}:{}", action_offset, step_offset);
        render_step(&step, renderer, &header, false, detail)
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
            let preamble = match prompt {
                Some(ref s) => format!("{}\n", s),
                None => "".to_string(),
            };
            let raw_prompt =
                format! {"{}Please fix the following errors: {}\n", preamble, self.error};
            let new_step = Step::new(model, raw_prompt, StrategyStep::Code(CodeStep::new(prompt)));
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
                if action.steps.is_empty() {
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

    fn render<R: unirend::Render>(
        &self,
        _config: &Config,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
        renderer: &mut R,
        detail: Detail,
    ) -> Result<()> {
        let step = session.get_action(action_offset)?.steps[step_offset].clone();

        // Create the header
        let header = format!("Step {}", step_offset);

        render_step(&step, renderer, &header, true, detail)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{error::TenxError, strategy::Strategy, testutils::test_project};

    #[test]
    fn test_code_next_step() -> Result<()> {
        let test_project = test_project();
        let code = Code::new();
        let mut session = Session::new(&test_project.config)?;

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
            StrategyStep::Code(CodeStep::default()),
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
        let fix = Fix::new("error");

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
        assert!(session
            .last_step()
            .unwrap()
            .raw_prompt
            .starts_with("Fix prompt"));

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
