use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    config::Config,
    error::{Result, TenxError},
    events::{send_event, Event, EventSender},
    session::{Session, Step, StepType},
};

use super::core::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Code {
    pub prompt: String,
}

impl Code {
    pub fn new(prompt: String) -> Self {
        Self { prompt }
    }
}

/// Common logic for handling steps in both Code and Fix strategies
fn handle_existing_step(
    config: &Config,
    step: &Step,
    events: Option<EventSender>,
) -> Result<Option<Step>> {
    let model = config.models.default.clone();
    if let Some(err) = &step.err {
        if let Some(model_message) = err.should_retry() {
            send_event(
                &events,
                Event::NextStep {
                    user: format!("{}", err),
                    model: model_message.to_string(),
                },
            )?;
            debug!("Next step, based on error: {}", err);

            return Ok(Some(Step::new(
                model,
                model_message.to_string(),
                StepType::Error,
            )));
        }
    } else if let Some(model_response) = &step.model_response {
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

            return Ok(Some(Step::new(model, model_message, StepType::Auto)));
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
    ) -> Result<Option<Step>> {
        if let Some(action) = session.last_action() {
            if let Some(step) = action.last_step() {
                return handle_existing_step(config, step, events);
            } else {
                let model = config.models.default.clone();
                return Ok(Some(Step::new(model, self.prompt.clone(), StepType::Auto)));
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    error: TenxError,
    prompt: Option<String>,
}

impl Fix {
    pub fn new(error: TenxError, prompt: Option<String>) -> Self {
        Self { error, prompt }
    }
}

impl ActionStrategy for Fix {
    fn next_step(
        &self,
        config: &Config,
        session: &Session,
        events: Option<EventSender>,
    ) -> Result<Option<Step>> {
        if let Some(action) = session.last_action() {
            if let Some(step) = action.last_step() {
                return handle_existing_step(config, step, events);
            } else {
                let model = config.models.default.clone();
                let prompt = self
                    .prompt
                    .clone()
                    .unwrap_or_else(|| "Please fix the following errors.".to_string());
                return Ok(Some(Step::new(model, prompt, StepType::Error)));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testutils::test_project;

    #[test]
    fn test_code_next_step() -> Result<()> {
        let test_project = test_project();
        let code = Code::new("Test".into());
        let mut session = Session::default();

        assert!(code
            .next_step(&test_project.config, &session, None)?
            .is_none());

        session.add_action(super::super::Strategy::Code(code.clone()))?;
        let step = code
            .next_step(&test_project.config, &session, None)?
            .unwrap();
        assert_eq!(step.prompt, "Test");
        assert_eq!(step.step_type, StepType::Auto);

        session.add_step(
            test_project.config.models.default.clone(),
            "Test".into(),
            StepType::Auto,
        )?;
        let patch_err = TenxError::Patch {
            user: "Error".into(),
            model: "Retry".into(),
        };
        session.last_step_mut().unwrap().err = Some(patch_err);
        let step = code
            .next_step(&test_project.config, &session, None)?
            .unwrap();
        assert_eq!(step.prompt, "Retry");
        assert_eq!(step.step_type, StepType::Error);

        session.last_step_mut().unwrap().err = Some(TenxError::Config("Error".into()));
        assert!(code
            .next_step(&test_project.config, &session, None)?
            .is_none());

        Ok(())
    }

    #[test]
    fn test_fix_next_step() -> Result<()> {
        let test_project = test_project();
        let mut session = Session::default();

        // Empty session
        let fix = Fix::new(TenxError::Config("Error".into()), Some("Fix prompt".into()));
        assert!(fix
            .next_step(&test_project.config, &session, None)?
            .is_none());

        // Custom prompt
        session.add_action(super::super::Strategy::Fix(fix))?;
        let step = session
            .last_action()
            .unwrap()
            .strategy
            .next_step(&test_project.config, &session, None)?
            .unwrap();
        assert_eq!(step.prompt, "Fix prompt");
        assert_eq!(step.step_type, StepType::Error);

        // Retryable error
        session.add_step(
            test_project.config.models.default.clone(),
            "Test".into(),
            StepType::Auto,
        )?;
        session.last_step_mut().unwrap().err = Some(TenxError::Patch {
            user: "Error".into(),
            model: "Retry".into(),
        });
        let step = session
            .last_action()
            .unwrap()
            .strategy
            .next_step(&test_project.config, &session, None)?
            .unwrap();
        assert_eq!(step.prompt, "Retry");
        assert_eq!(step.step_type, StepType::Error);

        // Default prompt
        let fix = Fix::new(TenxError::Config("Error".into()), None);
        session.add_action(super::super::Strategy::Fix(fix))?;
        let step = session
            .last_action()
            .unwrap()
            .strategy
            .next_step(&test_project.config, &session, None)?
            .unwrap();
        assert_eq!(step.prompt, "Please fix the following errors.");

        Ok(())
    }
}
