pub mod builtin;
pub mod shell;

pub use builtin::*;

use crate::{
    config::Config,
    lang::{python::*, rust::*},
    Result, Session,
};

/// The mode in which the check should run - preflight, post-patch or both.
pub enum Mode {
    Pre,
    Post,
    Both,
}

pub enum Runnable {
    Ok,
    Error(String),
}

impl Runnable {
    pub fn is_ok(&self) -> bool {
        matches!(self, Runnable::Ok)
    }
}

pub trait Check {
    /// Returns the name of the check.
    fn name(&self) -> String;

    /// Performs a check on the given PromptInput and State.
    fn check(&self, config: &Config, state: &Session) -> Result<()>;

    /// Determines if the check should run for the given session. If editables are specified in
    /// the session, the check will only run if the session's editables contain a relevant
    /// file. Otherwise, the check will run if any of the project included files are relevant.
    fn is_relevant(&self, config: &Config, state: &Session) -> Result<bool>;

    /// Checks if the check can be run.
    fn runnable(&self) -> Result<Runnable>;

    /// Returns true if this check is disabled by default.
    fn default_off(&self) -> bool {
        true
    }

    fn mode(&self) -> Mode {
        Mode::Both
    }
}

/// Returns a vector of all available checks.
pub fn all_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(RustCargoCheck),
        Box::new(RustCargoTest),
        Box::new(RustCargoClippy),
        Box::new(PythonRuffCheck),
    ]
}

/// Returns a list of checks based on the given prompt and state.
pub fn relevant_checks(config: &Config, state: &Session) -> Result<Vec<Box<dyn Check>>> {
    let mut checks: Vec<Box<dyn Check>> = Vec::new();
    for checker in all_checks() {
        if checker.is_relevant(config, state)? && checker.runnable()?.is_ok() {
            checks.push(checker);
        }
    }
    Ok(checks)
}
