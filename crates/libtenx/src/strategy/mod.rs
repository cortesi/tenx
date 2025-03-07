use serde::{Deserialize, Serialize};

pub mod code;
mod core;

use crate::{
    checks::CheckMode, config::Config, error::Result, events::EventSender, session::Session,
};

pub use code::*;
pub use core::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Strategy {
    Code(Code),
    Fix(Fix),
}

impl ActionStrategy for Strategy {
    /// The name of this strategy.
    fn name(&self) -> &'static str {
        match self {
            Strategy::Code(code) => code.name(),
            Strategy::Fix(fix) => fix.name(),
        }
    }

    fn next_step(
        &self,
        config: &Config,
        session: &mut Session,
        action_offset: usize,
        sender: Option<EventSender>,
        prompt: Option<String>,
    ) -> Result<ActionState> {
        match self {
            Strategy::Code(code) => code.next_step(config, session, action_offset, sender, prompt),
            Strategy::Fix(fix) => fix.next_step(config, session, action_offset, sender, prompt),
        }
    }

    /// The current action state for this action.
    fn state(&self, config: &Config, session: &Session, action_offset: usize) -> ActionState {
        match self {
            Strategy::Code(code) => code.state(config, session, action_offset),
            Strategy::Fix(fix) => fix.state(config, session, action_offset),
        }
    }

    /// Run the checks for this strategy.
    fn check(
        &self,
        config: &Config,
        session: &mut Session,
        sender: Option<EventSender>,
        mode: CheckMode,
    ) -> Result<()> {
        match self {
            Strategy::Code(code) => code.check(config, session, sender, mode),
            Strategy::Fix(fix) => fix.check(config, session, sender, mode),
        }
    }
}
