mod rust;
pub use rust::*;

use crate::{PromptInput, Result, Session};

pub trait Validator {
    /// Performs a check on the given PromptInput and State.
    fn validate(&self, prompt: &PromptInput, state: &Session) -> Result<()>;
}

/// Returns a list of preflight checkers based on the given prompt and state.
pub fn preflight(_prompt: &PromptInput, _state: &Session) -> Vec<Box<dyn Validator>> {
    vec![]
}
