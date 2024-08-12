mod rust;
pub use rust::*;

use crate::{PromptInput, Result, State};

pub trait Checker {
    /// Performs a check on the given PromptInput and State.
    fn check(&self, prompt: &PromptInput, state: &State) -> Result<()>;
}

/// Returns a list of preflight checkers based on the given prompt and state.
pub fn preflight(_prompt: &PromptInput, _state: &State) -> Vec<Box<dyn Checker>> {
    vec![]
}
