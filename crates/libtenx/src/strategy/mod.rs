use serde::{Deserialize, Serialize};

pub mod code;
mod core;

use crate::{
    config::Config,
    error::Result,
    events::EventSender,
    session::{Session, Step},
};

pub use code::*;
pub use core::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Strategy {
    Code(Code),
    Fix(Fix),
}

impl ActionStrategy for Strategy {
    fn next_step(
        &self,
        config: &Config,
        session: &Session,
        sender: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<Option<Step>> {
        match self {
            Strategy::Code(code) => code.next_step(config, session, sender, prompt),
            Strategy::Fix(fix) => fix.next_step(config, session, sender, prompt),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Strategy::Code(code) => code.name(),
            Strategy::Fix(fix) => fix.name(),
        }
    }

    fn state(&self, config: &Config, session: &Session) -> State {
        match self {
            Strategy::Code(code) => code.state(config, session),
            Strategy::Fix(fix) => fix.state(config, session),
        }
    }
}
