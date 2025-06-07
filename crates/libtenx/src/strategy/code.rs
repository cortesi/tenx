use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    checks::CheckResult,
    checks::{check_all, check_paths},
    config::Config,
    context::ContextProvider,
    error::Result,
    error::TenxError,
    events::EventSender,
    model::Chat,
    session::{Action, Step},
};
use state::Operation;
use unirend::{Detail, Render, Style};

use super::*;

pub const ACK: &str = "Got it.";

fn build_chat(
    config: &Config,
    session: &Session,
    action_offset: usize,
    chat: &mut Box<dyn Chat>,
) -> Result<()> {
    if !session.contexts.is_empty() {
        for cspec in &session.contexts {
            for ctx in cspec.context_items(config, session)? {
                chat.add_context(&ctx)?;
            }
        }
        chat.add_agent_message(ACK)?;
    }

    let action = session
        .actions
        .get(action_offset)
        .ok_or(TenxError::Internal(format!(
            "Action {action_offset} not found in session",
        )))?;
    for (step_offset, step) in action.steps.iter().enumerate() {
        if step_offset > 0 {
            if let Some(prev_step) = action.steps.get(step_offset - 1) {
                if let Some(model_response) = &prev_step.model_response {
                    if let Some(comment) = &model_response.comment {
                        chat.add_agent_message(comment)?;
                    }
                    if let Some(patch) = &model_response.patch {
                        chat.add_agent_patch(patch)?;
                    }

                    if let Some(patch) = &model_response.patch {
                        for op in &patch.ops {
                            match op {
                                Operation::View(path) => {
                                    chat.add_editable(
                                        path.as_os_str().to_str().unwrap_or_default(),
                                        &action.state.read(path)?,
                                    )?;
                                }
                                Operation::ViewRange(path, start, end) => {
                                    chat.add_editable(
                                        path.as_os_str().to_str().unwrap_or_default(),
                                        &action.state.read_range(path, *start, *end)?,
                                    )?;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                chat.add_user_check_results(&prev_step.check_results)?;
                if let Some(patch_info) = &prev_step.patch_info {
                    chat.add_user_patch_failure(&patch_info.failures)?;
                }
            }
        } else if let StrategyState::Fix(f) = &step.strategy_state {
            chat.add_user_check_results(&f.check_results)?;
        }
        if !step.prompt.is_empty() {
            chat.add_user_prompt(&step.prompt)?;
        }
    }

    Ok(())
}

/// Shared step data for Code and Fix strategies.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CodeState {}

/// Shared step data for Code and Fix strategies.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct FixState {
    pub check_results: Vec<CheckResult>,
}

/// The Code strategy allows the model to write and modify code based on a prompt.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Code {}

/// The Fix strategy is used to resolve errors in code by providing the model with error details.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Fix {}

/// Common logic for processing the next step in both Code and Fix actions.
///
/// This function:
/// 1. Checks for errors and patch failures in the current step
/// 2. Creates a new step with appropriate messages if needed
/// 3. Returns the current state of the action
fn next_step(
    _config: &Config,
    session: &mut Session,
    action_offset: usize,
    _events: Option<EventSender>,
) -> Result<ActionState> {
    let last_step = session.actions[action_offset]
        .last_step()
        .ok_or(TenxError::Internal("No steps in action".into()))?;
    if !should_next_step(last_step) {
        debug!("Action complete - no next step needed");
        return Ok(ActionState {
            completion: Completion::Complete,
            input_required: InputRequired::No,
        });
    }

    let new_step = Step::new(
        last_step.model.clone(),
        StrategyState::Code(CodeState::default()),
    );
    session.last_action_mut()?.add_step(new_step)?;

    debug!("Action: next step created: {:#?}", session.last_step());
    Ok(ActionState {
        completion: Completion::Incomplete,
        input_required: InputRequired::No,
    })
}

/// Common check function
fn check(
    config: &Config,
    session: &mut Session,
    action_offset: usize,
    events: Option<EventSender>,
) -> Result<()> {
    let last_step = session.actions[action_offset]
        .last_step()
        .ok_or(TenxError::Internal("No steps in action".into()))?;
    if session.actions[action_offset]
        .state
        .was_modified_since(last_step.rollback_id)
    {
        let paths = session.actions[action_offset].state.changed()?;
        if paths.is_empty() {
            // No changes, nothing to check
            return Ok(());
        }
        let check_results = check_paths(config, &paths, &events)?;
        if let Some(last_step) = session.last_step_mut() {
            last_step.check_results = check_results;
        }
    }
    Ok(())
}

/// Should a next step be generated? True if:
///
/// - There is a check failure
/// - The patch included view requests
/// - The patch has failures
/// - There is a retryable error in the last step
///
pub fn should_next_step(step: &Step) -> bool {
    // Check failure
    if !step.check_results.is_empty() {
        return true;
    }

    if let Some(patch_info) = &step.patch_info {
        // Patch included view requests
        if patch_info.should_continue {
            return true;
        }
        // Patch has failures
        if !patch_info.failures.is_empty() {
            return true;
        }
    };

    // Retryable error
    if let Some(err) = &step.err {
        if err.should_retry().is_some() {
            return true;
        }
    }

    false
}

/// Determines the current state of an action
fn get_action_state(action: &Action) -> ActionState {
    if action.steps.is_empty() {
        return ActionState {
            completion: Completion::Incomplete,
            input_required: InputRequired::Yes,
        };
    }
    let last_step = action.last_step().unwrap();
    if last_step.model_response.is_none() {
        ActionState {
            completion: Completion::Incomplete,
            input_required: InputRequired::No,
        }
    } else if !should_next_step(last_step) {
        ActionState {
            completion: Completion::Complete,
            input_required: InputRequired::No,
        }
    } else {
        ActionState {
            completion: Completion::Incomplete,
            input_required: InputRequired::No,
        }
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

    if detail == Detail::Full {
        renderer.push("raw prompt");
        renderer.para(&step.prompt);
        renderer.pop();
        if let Some(model_response) = &step.model_response {
            if let Some(raw_response) = &model_response.raw_response {
                renderer.push("raw response");
                renderer.para(raw_response);
                renderer.pop();
            }
        }
    } else if let Some(model_response) = &step.model_response {
        if let Some(comment) = &model_response.comment {
            renderer.push("model comment");
            renderer.para(comment);
            renderer.pop();
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
                .map(|failure| format!("Failed to apply {:?}: {}", failure.operation, failure.user))
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

#[async_trait]
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
        check(config, session, action_offset, events)
    }

    fn next_step(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        events: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<ActionState> {
        let action = &session.actions[action_offset];
        if let Some(step) = action.last_step() {
            // If the last step is incomplete, don't synthesize a new step
            if step.is_incomplete() {
                return Ok(ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::No,
                });
            }
            next_step(config, session, action_offset, events)
        } else if let Some(p) = prompt {
            // First step, with a provided prompt
            let model = config.models.default.clone();
            let new_step =
                Step::new(model, StrategyState::Code(CodeState::default())).with_prompt(p);
            session.last_action_mut()?.add_step(new_step)?;
            Ok(ActionState {
                completion: Completion::Incomplete,
                input_required: InputRequired::No,
            })
        } else {
            // No prompt, need user input
            Ok(ActionState {
                completion: Completion::Incomplete,
                input_required: InputRequired::Yes,
            })
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
        let step = &session.actions[action_offset].steps[step_offset].clone();
        let header = format!("step {action_offset}:{step_offset}");
        render_step(step, renderer, &header, false, detail)
    }

    async fn send(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        sender: Option<EventSender>,
    ) -> Result<ModelResponse> {
        let model = config.active_model()?;
        let mut chat = model
            .chat()
            .ok_or(TenxError::Internal("Chat not supported".into()))?;
        build_chat(config, session, action_offset, &mut chat)?;
        chat.send(sender).await
    }
}

#[async_trait]
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
        check(config, session, action_offset, events)
    }

    fn next_step(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        events: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<ActionState> {
        let action = &mut session.actions[action_offset];
        if let Some(step) = action.last_step() {
            if step.is_incomplete() {
                return Ok(ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::No,
                });
            }
            next_step(config, session, action_offset, events)
        } else {
            // First step in the action, let's run the tests
            let check_results = check_all(config, &events)?;
            Ok(if check_results.is_empty() {
                ActionState {
                    completion: Completion::Complete,
                    input_required: InputRequired::No,
                }
            } else {
                let model = config.models.default.clone();
                let preamble = match prompt {
                    Some(ref s) => format!("{s}\n"),
                    None => "".to_string(),
                };
                let raw_prompt = format! {"{preamble}\nPlease fix the following errors.\n"};
                let new_step = Step::new(model, StrategyState::Fix(FixState { check_results }))
                    .with_prompt(raw_prompt);
                action.add_step(new_step)?;
                ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::No,
                }
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
        let step = &session.actions[action_offset].steps[step_offset].clone();

        // Create the header
        let header = format!("Step {step_offset}");

        render_step(step, renderer, &header, true, detail)
    }

    async fn send(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        sender: Option<EventSender>,
    ) -> Result<ModelResponse> {
        let model = config.active_model()?;
        let mut chat = model
            .chat()
            .ok_or(TenxError::Internal("Chat not supported".into()))?;

        let dialect = Tags::new();
        dialect.build_chat(config, session, action_offset, &mut chat)?;
        chat.send(sender).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{strategy::Strategy, testutils::test_project};
    use state;
    use std::path::PathBuf;

    #[test]
    fn test_code_next_step() -> Result<()> {
        let test_project = test_project();
        let code = Code::default();
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
        assert_eq!(session_clone.last_step().unwrap().prompt, "Test");

        // Test retry with patch failure
        session.last_action_mut()?.add_step(
            Step::new(
                test_project.config.models.default.clone(),
                StrategyState::Code(CodeState::default()),
            )
            .with_prompt("Test"),
        )?;

        // Simulate a patch failure
        session.last_step_mut().unwrap().patch_info = Some(state::PatchInfo {
            rollback_id: 0,
            succeeded: 0,
            failures: vec![state::PatchFailure {
                user: "Text not found".into(),
                model: "The text 'old_text' was not found in the file".into(),
                operation: state::Operation::Replace(state::Replace {
                    path: PathBuf::from("test.rs"),
                    old: "old_text".into(),
                    new: "new_text".into(),
                }),
            }],
            should_continue: false,
        });

        let state = code.next_step(&test_project.config, &mut session, action_idx, None, None)?;
        assert_eq!(state.completion, Completion::Incomplete);
        assert_eq!(state.input_required, InputRequired::No);

        // Test retryable error
        let mut session_clone = session.clone();
        session_clone.last_step_mut().unwrap().err = Some(TenxError::ResponseParse {
            user: "Failed to parse response".into(),
            model: "Please format your response correctly".into(),
        });
        session_clone.last_step_mut().unwrap().patch_info = None;

        let state = code.next_step(
            &test_project.config,
            &mut session_clone,
            action_idx,
            None,
            None,
        )?;
        assert_eq!(state.completion, Completion::Incomplete);

        // Non-retryable error should complete the action
        let mut session_clone2 = session.clone();
        session_clone2.last_step_mut().unwrap().err = Some(TenxError::Config("Error".into()));
        session_clone2.last_step_mut().unwrap().patch_info = None;

        let state = code.next_step(
            &test_project.config,
            &mut session_clone2,
            action_idx,
            None,
            None,
        )?;
        assert_eq!(state.completion, Completion::Complete);

        Ok(())
    }

    #[test]
    fn test_fix_next_step() -> Result<()> {
        let test_project = test_project().with_check_result(Some(Ok(vec![CheckResult {
            name: "test".into(),
            user: "Test error".into(),
            model: "Model response".into(),
        }])));

        let mut session = Session::new(&test_project.config)?;
        let fix = Fix::default();

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

        assert!(session.actions[action_idx]
            .last_step()
            .unwrap()
            .prompt
            .contains("Fix prompt"));

        // Test patch failure triggers retry
        session.last_step_mut().unwrap().patch_info = Some(state::PatchInfo {
            rollback_id: 0,
            succeeded: 1,
            failures: vec![state::PatchFailure {
                user: "Unable to find text".into(),
                model: "The search pattern was not found".into(),
                operation: state::Operation::Replace(state::Replace {
                    path: PathBuf::from("main.rs"),
                    old: "pattern".into(),
                    new: "replacement".into(),
                }),
            }],
            should_continue: false,
        });

        let state = fix.next_step(&test_project.config, &mut session, action_idx, None, None)?;
        assert_eq!(state.completion, Completion::Incomplete);
        assert_eq!(state.input_required, InputRequired::No);

        // Test default prompt in a new action
        let fix2 = Fix::default();
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
            .prompt
            .starts_with("\nPlease fix the following errors"));

        Ok(())
    }
}
