use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::Result,
    events::EventSender,
    session::{Session, Step},
};

/// Is the current action complete?
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Completion {
    /// The action is complete.
    Complete,

    /// The action is not complete.
    Incomplete,

    /// The action is complete, but can continue if requested.
    CompleteContinue,
}

impl Completion {
    pub fn is_complete(&self) -> bool {
        matches!(self, Completion::Complete | Completion::CompleteContinue)
    }
}

/// Is user input required to create the next step?
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InputRequired {
    /// User input is mandatory to generate the next step.
    Yes,

    /// User input is invalid to generate the next step.
    No,

    /// User input is optional to generate the next step.
    Optional,
}

/// The state of the current action.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionState {
    /// Is the action complete?
    pub completion: Completion,

    /// Is user input required to create the next step?
    pub input_required: InputRequired,
}

impl ActionState {
    /// Should the action stop iteration?
    pub fn should_stop_iteration(&self) -> bool {
        self.input_required == InputRequired::Yes || self.completion.is_complete()
    }
}

pub trait ActionStrategy {
    /// Returns the name of the strategy.
    fn name(&self) -> &'static str;

    /// Given a session and action offset, calculate the next step. This may involve complex actions like executing
    /// checks, making external requests. The returned step is ready to be sent to the upstream
    /// model. The action's steps may currently be empty, in which case the first step is
    /// synthesized. The strategy may choose to leave the current steps intact, or to edit the
    /// current step history to progress.
    ///
    /// If the action is complete, return None.
    ///
    /// * `action_offset` - The index of the action in the session's actions list
    fn next_step(
        &self,
        config: &Config,
        session: &Session,
        action_offset: usize,
        sender: Option<EventSender>,
        user_input: Option<String>,
    ) -> Result<Option<Step>>;

    /// Returns the current state of the action, including completion status and input requirements.
    ///
    /// * `action_offset` - The index of the action in the session's actions list
    fn state(&self, _config: &Config, _session: &Session, _action_offset: usize) -> ActionState {
        ActionState {
            completion: Completion::Incomplete,
            input_required: InputRequired::No,
        }
    }
}
