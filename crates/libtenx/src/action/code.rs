use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::{Result, TenxError},
    events::EventSender,
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

impl ActionStrategy for Code {
    fn next_step(
        &self,
        config: &Config,
        session: &Session,
        _events: Option<EventSender>,
    ) -> Result<Option<Step>> {
        if let Some(action) = session.last_action() {
            if let Some(step) = action.last_step() {
                if let Some(err) = &step.err {
                    if let Some(model_message) = err.should_retry() {
                        let model = config.models.default.clone();
                        return Ok(Some(Step::new(
                            model,
                            model_message.to_string(),
                            StepType::Error,
                        )));
                    }
                }
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
        _config: &Config,
        _session: &Session,
        _events: Option<EventSender>,
    ) -> Result<Option<Step>> {
        Ok(None)
    }
}
