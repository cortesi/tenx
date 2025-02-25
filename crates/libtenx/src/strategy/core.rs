use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::Result,
    events::EventSender,
    session::{Session, Step},
};

/// The type of user input required for the next step.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InputRequired {
    Yes,
    No,
    Optional,
}

pub trait ActionStrategy {
    /// Returns the name of the strategy.
    fn name(&self) -> &'static str;

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
    ) -> Result<Option<Step>>;

    /// Does generating the next step require user input?
    fn input_required(&self, _config: &Config, _session: &Session) -> InputRequired {
        InputRequired::No
    }
}
