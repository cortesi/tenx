mod rust;
pub use rust::*;

use crate::{PromptInput, Result, State};

pub trait Validator {
    /// Performs a check on the given PromptInput and State.
    fn validate(&self, prompt: &PromptInput, state: &State) -> Result<()>;
}

/// Returns a list of preflight checkers based on the given prompt and state.
pub fn preflight(_prompt: &PromptInput, _state: &State) -> Vec<Box<dyn Validator>> {
    vec![]
}
