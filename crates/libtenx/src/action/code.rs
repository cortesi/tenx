use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    events::EventSender,
    session::{Session, Step},
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
        _config: &Config,
        _session: &Session,
        _events: Option<EventSender>,
    ) -> Option<Step> {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    prompt: Option<String>,
}

impl Fix {
    pub fn new(prompt: Option<String>) -> Self {
        Self { prompt }
    }
}

impl ActionStrategy for Fix {
    fn next_step(
        &self,
        _config: &Config,
        _session: &Session,
        _events: Option<EventSender>,
    ) -> Option<Step> {
        None
    }
}
