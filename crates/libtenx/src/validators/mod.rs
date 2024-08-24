mod rust;
pub use rust::*;

use crate::{Result, Session};

pub trait Validator {
    /// Returns the name of the validator.
    fn name(&self) -> &'static str;

    /// Performs a check on the given PromptInput and State.
    fn validate(&self, state: &Session) -> Result<()>;
}

/// Returns a list of preflight checkers based on the given prompt and state.
pub fn preflight(state: &Session) -> Result<Vec<Box<dyn Validator>>> {
    let mut validators: Vec<Box<dyn Validator>> = vec![];
    if state
        .editables()?
        .iter()
        .any(|path| path.extension().map_or(false, |ext| ext == "rs"))
    {
        validators.push(Box::new(CargoChecker));
    }

    Ok(validators)
}

/// Returns a list of post-patch checkers based on the given state.
pub fn post_patch(state: &Session) -> Result<Vec<Box<dyn Validator>>> {
    let mut validators: Vec<Box<dyn Validator>> = vec![];
    if let Some(last_step) = state.steps().last() {
        if let Some(patch) = &last_step.patch {
            if patch
                .changed_files()
                .iter()
                .any(|path| path.extension().map_or(false, |ext| ext == "rs"))
            {
                validators.push(Box::new(CargoChecker));
            }
        }
    }
    Ok(validators)
}
