use serde::{Deserialize, Serialize};

use super::code::{Code, Fix};
use crate::{
    config::Config,
    events::EventSender,
    session::{Session, Step},
};

pub trait ActionStrategy {
    /// Given a session, calculate the next step. This may involve complex actions like executing
    /// checks, making external requests, asking for user input. The returned step is ready to be
    /// sent to the upstream model. The action's steps my currently be empty, in which case the
    /// first step is synthesized.
    ///
    /// If the action is complete, return None. The current action is presumed to be the last one
    /// in the session.
    fn next_step(
        &self,
        config: &Config,
        session: &Session,
        sender: Option<EventSender>,
    ) -> Option<Step>;
}

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
    ) -> Option<Step> {
        match self {
            Strategy::Code(code) => code.next_step(config, session, sender),
            Strategy::Fix(fix) => fix.next_step(config, session, sender),
        }
    }
}
