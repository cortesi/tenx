pub mod rust;
pub use rust::*;

use crate::{config::Config, Result, Session};

pub trait Validator {
    /// Returns the name of the validator.
    fn name(&self) -> &'static str;

    /// Performs a check on the given PromptInput and State.
    fn validate(&self, state: &Session) -> Result<()>;

    /// Determines if the validator should run for the given session.
    fn is_relevant(&self, config: &Config, state: &Session) -> Result<bool>;
}

/// Returns a vector of all available validators.
pub fn all_validators() -> Vec<Box<dyn Validator>> {
    vec![
        Box::new(RustCargoCheck),
        Box::new(RustCargoTest),
        Box::new(RustCargoClippy),
    ]
}

/// Returns a list of validators based on the given prompt and state.
pub fn relevant_validators(config: &Config, state: &Session) -> Result<Vec<Box<dyn Validator>>> {
    let mut validators: Vec<Box<dyn Validator>> = Vec::new();
    for checker in all_validators() {
        if checker.is_relevant(config, state)? {
            validators.push(checker);
        }
    }
    Ok(validators)
}
