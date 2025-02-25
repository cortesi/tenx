use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    config::Config,
    error::{Result, TenxError},
    events::{send_event, Event, EventSender},
    session::{Session, Step},
};

use super::core::*;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Code {}

impl Code {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Common logic for the last step in both Code and Fix strategies. We either synthesize the next
/// step, or return None if we're done.
fn next_step(config: &Config, step: &Step, events: Option<EventSender>) -> Result<Option<Step>> {
    let model = config.models.default.clone();
    let mut messages = Vec::new();
    let mut user_message = Vec::new();

    // Check for errors in the step
    if let Some(err) = &step.err {
        if let Some(err_message) = err.should_retry() {
            messages.push(err_message.to_string());
            user_message.push(format!("{}", err));
        }
    }

    // Check for patch_info failures
    if let Some(patch_info) = &step.patch_info {
        if !patch_info.failures.is_empty() {
            let failure_messages: Vec<String> = patch_info
                .failures
                .iter()
                .map(|(change, err)| format!("Failed to apply change {:?}: {}", change, err))
                .collect();

            messages.push(format!(
                "Please fix the following issues with your changes:\n\n{}",
                failure_messages.join("\n\n")
            ));
            user_message.push("patch failures".into());
        }
    }

    // If we have errors or patch failures, send a combined message
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
        return Ok(Some(Step::new(model, model_message)));
    }

    // Check for operations in model response
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
            return Ok(Some(Step::new(model, model_message)));
        }
    }
    Ok(None)
}

impl ActionStrategy for Code {
    fn next_step(
        &self,
        config: &Config,
        session: &Session,
        events: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<Option<Step>> {
        if let Some(action) = session.last_action() {
            if let Some(step) = action.last_step() {
                return next_step(config, step, events);
            } else {
                // Synthesize first step in the action
                if let Some(p) = prompt {
                    let model = config.models.default.clone();
                    return Ok(Some(Step::new(model, p)));
                } else {
                    return Err(TenxError::Internal(
                        "No prompt provided for code action".to_string(),
                    ));
                }
            }
        }
        Ok(None)
    }

    fn name(&self) -> &'static str {
        "code"
    }

    fn state(&self, _config: &Config, session: &Session) -> State {
        if let Some(action) = session.last_action() {
            if action.steps().is_empty() {
                return State {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::Yes,
                };
            }

            // Check if the last step completed without errors
            if let Some(step) = action.last_step() {
                let has_errors = step.err.is_some()
                    || step
                        .patch_info
                        .as_ref()
                        .is_some_and(|p| !p.failures.is_empty());

                if !has_errors {
                    return State {
                        completion: Completion::Complete,
                        input_required: InputRequired::No,
                    };
                }
            }
        }

        State {
            completion: Completion::Incomplete,
            input_required: InputRequired::No,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    error: TenxError,
}

impl Fix {
    pub fn new(error: TenxError) -> Self {
        Self { error }
    }
}

impl ActionStrategy for Fix {
    fn name(&self) -> &'static str {
        "fix"
    }

    fn next_step(
        &self,
        config: &Config,
        session: &Session,
        events: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<Option<Step>> {
        if let Some(action) = session.last_action() {
            if let Some(step) = action.last_step() {
                return next_step(config, step, events);
            } else {
                // Synthesize first step in the action
                let model = config.models.default.clone();
                let default_prompt = "Please fix the following errors.".to_string();
                return Ok(Some(Step::new(model, prompt.unwrap_or(default_prompt))));
            }
        }
        Ok(None)
    }

    fn state(&self, _config: &Config, session: &Session) -> State {
        if let Some(action) = session.last_action() {
            if action.steps().is_empty() {
                return State {
                    completion: Completion::Incomplete,
                    input_required: InputRequired::Optional,
                };
            }

            // Check if the last step completed without errors
            if let Some(step) = action.last_step() {
                let has_errors = step.err.is_some()
                    || step
                        .patch_info
                        .as_ref()
                        .is_some_and(|p| !p.failures.is_empty());

                if !has_errors {
                    return State {
                        completion: Completion::Complete,
                        input_required: InputRequired::No,
                    };
                }
            }
        }

        State {
            completion: Completion::Incomplete,
            input_required: InputRequired::No,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::session::Action;
    use crate::strategy::Strategy;
    use crate::testutils::test_project;

    #[test]
    fn test_code_next_step() -> Result<()> {
        let test_project = test_project();
        let code = Code::new();
        let mut session = Session::new(&test_project.config)?;

        assert!(code
            .next_step(&test_project.config, &session, None, None)?
            .is_none());

        session.add_action(Action::new(
            &test_project.config,
            Strategy::Code(code.clone()),
        )?)?;

        // Should fail without a prompt
        assert!(code
            .next_step(&test_project.config, &session, None, None)
            .is_err());

        // With prompt
        let step = code
            .next_step(&test_project.config, &session, None, Some("Test".into()))?
            .unwrap();
        assert_eq!(step.raw_prompt, "Test");

        session.add_step(Step::new(
            test_project.config.models.default.clone(),
            "Test".into(),
        ))?;
        let patch_err = TenxError::Patch {
            user: "Error".into(),
            model: "Retry".into(),
        };
        session.last_step_mut().unwrap().err = Some(patch_err);
        let step = code
            .next_step(&test_project.config, &session, None, None)?
            .unwrap();
        assert_eq!(step.raw_prompt, "Retry");

        session.last_step_mut().unwrap().err = Some(TenxError::Config("Error".into()));
        assert!(code
            .next_step(&test_project.config, &session, None, None)?
            .is_none());

        Ok(())
    }

    #[test]
    fn test_fix_next_step() -> Result<()> {
        let test_project = test_project();
        let mut session = Session::new(&test_project.config)?;

        // Empty session
        let fix = Fix::new(TenxError::Config("Error".into()));
        assert!(fix
            .next_step(&test_project.config, &session, None, None)?
            .is_none());

        // Custom prompt
        session.add_action(Action::new(&test_project.config, Strategy::Fix(fix))?)?;
        let step = session
            .last_action()
            .unwrap()
            .strategy
            .next_step(
                &test_project.config,
                &session,
                None,
                Some("Fix prompt".into()),
            )?
            .unwrap();
        assert_eq!(step.raw_prompt, "Fix prompt");

        // Retryable error
        session.add_step(Step::new(
            test_project.config.models.default.clone(),
            "Test".into(),
        ))?;
        session.last_step_mut().unwrap().err = Some(TenxError::Patch {
            user: "Error".into(),
            model: "Retry".into(),
        });
        let step = session
            .last_action()
            .unwrap()
            .strategy
            .next_step(&test_project.config, &session, None, None)?
            .unwrap();
        assert_eq!(step.raw_prompt, "Retry");

        // Default prompt
        let fix = Fix::new(TenxError::Config("Error".into()));
        session.add_action(Action::new(&test_project.config, Strategy::Fix(fix))?)?;
        let step = session
            .last_action()
            .unwrap()
            .strategy
            .next_step(&test_project.config, &session, None, None)?
            .unwrap();
        assert_eq!(step.raw_prompt, "Please fix the following errors.");

        Ok(())
    }
}
