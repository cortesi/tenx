pub use crate::lang::python::*;
pub use crate::lang::rust::*;

use crate::{config::Config, Result, Session};

pub enum Runnable {
    Ok,
    Error(String),
}

impl Runnable {
    pub fn is_ok(&self) -> bool {
        matches!(self, Runnable::Ok)
    }
}

pub trait Validator {
    /// Returns the name of the validator.
    fn name(&self) -> &'static str;

    /// Performs a check on the given PromptInput and State.
    fn validate(&self, state: &Session) -> Result<()>;

    /// Determines if the validator should run for the given session.
    fn is_relevant(&self, config: &Config, state: &Session) -> Result<bool>;

    /// Checks if the validator is configured to run.
    fn is_configured(&self, config: &Config) -> bool;

    /// Checks if the validator can be run.
    fn runnable(&self) -> Result<Runnable>;
}

/// Returns a vector of all available validators.
pub fn all_validators() -> Vec<Box<dyn Validator>> {
    vec![
        Box::new(RustCargoCheck),
        Box::new(RustCargoTest),
        Box::new(RustCargoClippy),
        Box::new(PythonRuffCheck),
    ]
}

/// Returns a list of validators based on the given prompt and state.
pub fn relevant_validators(config: &Config, state: &Session) -> Result<Vec<Box<dyn Validator>>> {
    let mut validators: Vec<Box<dyn Validator>> = Vec::new();
    for checker in all_validators() {
        if checker.is_configured(config)
            && checker.is_relevant(config, state)?
            && checker.runnable()?.is_ok()
        {
            validators.push(checker);
        }
    }
    Ok(validators)
}
