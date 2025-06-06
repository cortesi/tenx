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

    for (step_offset, step) in session.actions[action_offset].steps.iter().enumerate() {
        if let StrategyState::Fix(f) = &step.strategy_state {
            chat.add_user_check_results(&f.check_results)?;
        };

        if !step.prompt.is_empty() {
            chat.add_user_prompt(&step.prompt)?;
        }

        if step_offset > 0 {
            if let Some(prev_step) = session.actions[action_offset].steps.get(step_offset - 1) {
                chat.add_user_check_results(&prev_step.check_results)?;
                if let Some(patch_info) = &prev_step.patch_info {
                    chat.add_user_patch_failure(&patch_info.failures)?;
                }
            }
        }

        if let Some(model_response) = &step.model_response {
            if let Some(comment) = &model_response.comment {
                chat.add_agent_message(comment)?;
            }
            if let Some(patch) = &model_response.patch {
                chat.add_agent_patch(patch)?;
            }
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
    config: &Config,
    session: &mut Session,
    action_offset: usize,
    events: Option<EventSender>,
) -> Result<ActionState> {
    let last_step = session.actions[action_offset]
        .last_step()
        .ok_or(TenxError::Internal("No steps in action".into()))?;

    if let Some(err) = &last_step.err {
        if err.should_retry().is_none() {
            debug!("Action complete - non-retryable error: {err}");
            return Ok(ActionState {
                completion: Completion::Complete,
                input_required: InputRequired::No,
            });
        }
    } else if last_step.patch_info.is_none() {
        debug!("Action complete - no error or patch");
        return Ok(ActionState {
            completion: Completion::Complete,
            input_required: InputRequired::No,
        });
    }

    if session.actions[action_offset]
        .state
        .was_modified_since(last_step.rollback_id)
    {
        let paths = session.actions[action_offset].state.changed()?;
        if !paths.is_empty() {
            let check_results = check_paths(config, &paths, &events)?;
            if !check_results.is_empty() {
                let mut new_step = Step::new(
                    last_step.model.clone(),
                    StrategyState::Code(CodeState::default()),
                );
                new_step.check_results = check_results;
                session.last_action_mut()?.add_step(new_step)?;

                debug!("Action incomplete: next step created with check error");
                return Ok(ActionState {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::No,
                });
            }
        }
    }

    let new_step = Step::new(
        last_step.model.clone(),
        StrategyState::Code(CodeState::default()),
    );
    session.last_action_mut()?.add_step(new_step)?;

    debug!("Action incomplete: next step created");
    Ok(ActionState {
        completion: Completion::Incomplete,
        input_required: InputRequired::No,
    })
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
        let paths = session.actions[action_offset].state.changed()?;
        if paths.is_empty() {
            // No changes, nothing to check
            return Ok(());
        }
        let check_results = check_paths(config, &paths, &events)?;
        if let Some(last_step) = session.last_step_mut() {
            last_step.check_results = check_results;
        }
        Ok(())
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
            let model = config.models.default.clone();
            let new_step =
                Step::new(model, StrategyState::Code(CodeState::default())).with_prompt(p);
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
        let paths = &session.actions[action_offset].state.changed()?;
        let check_results = check_paths(config, paths, &events)?;
        if let Some(last_step) = session.last_step_mut() {
            last_step.check_results = check_results;
        }
        Ok(())
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
    use crate::{error::TenxError, strategy::Strategy, testutils::test_project};

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

        // Test retry with patch error
        session.last_action_mut()?.add_step(
            Step::new(
                test_project.config.models.default.clone(),
                StrategyState::Code(CodeState::default()),
            )
            .with_prompt("Test"),
        )?;
        let patch_err = TenxError::Patch {
            user: "Error".into(),
            model: "Retry".into(),
        };
        session.last_step_mut().unwrap().err = Some(patch_err);

        // let state = code.next_step(&test_project.config, &mut session, action_idx, None, None)?;
        // assert_eq!(state.completion, Completion::Incomplete);
        // assert_eq!(session.last_step().unwrap().raw_prompt, "Retry");
        //
        // // Non-retryable error should complete the action
        // let mut session_clone = session.clone();
        // session_clone.last_step_mut().unwrap().err = Some(TenxError::Config("Error".into()));
        //
        // let state = code.next_step(
        //     &test_project.config,
        //     &mut session_clone,
        //     action_idx,
        //     None,
        //     None,
        // )?;
        //
        // assert_eq!(state.completion, Completion::Complete);

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

        println!("Last step: {:#?}", session.actions[action_idx].steps);
        assert!(session.actions[action_idx]
            .last_step()
            .unwrap()
            .prompt
            .contains("Fix prompt"));

        //
        // // Test retryable error
        // session.last_step_mut().unwrap().err = Some(TenxError::Patch {
        //     user: "Error".into(),
        //     model: "Retry".into(),
        // });
        //
        // let state = fix.next_step(&test_project.config, &mut session, action_idx, None, None)?;
        // assert_eq!(state.completion, Completion::Incomplete);
        // assert_eq!(session.last_step().unwrap().raw_prompt, "Retry");
        //
        // // Test default prompt in a new action
        // let fix2 = Fix::default();
        // let mut session2 = Session::new(&test_project.config)?;
        // session2.add_action(Action::new(
        //     &test_project.config,
        //     Strategy::Fix(fix2.clone()),
        // )?)?;
        // let action_idx2 = session2.actions.len() - 1;
        //
        // let state = fix2.next_step(&test_project.config, &mut session2, action_idx2, None, None)?;
        // assert_eq!(state.completion, Completion::Incomplete);
        // assert!(session2
        //     .last_step()
        //     .unwrap()
        //     .raw_prompt
        //     .starts_with("Please fix the following errors"),);

        Ok(())
    }
}
