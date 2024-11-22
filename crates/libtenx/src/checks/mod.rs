pub mod builtin;
pub mod shell;

pub use builtin::*;

use crate::{config::Config, Result, Session};

/// The mode in which the check should run - preflight, post-patch or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Pre,
    Post,
    Both,
}

impl Mode {
    /// Returns true if this mode includes preflight checks.
    pub fn is_pre(&self) -> bool {
        matches!(self, Mode::Pre | Mode::Both)
    }

    /// Returns true if this mode includes post-patch checks.
    pub fn is_post(&self) -> bool {
        matches!(self, Mode::Post | Mode::Both)
    }
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

    /// Returns the glob patterns this check uses to determine relevance
    fn globs(&self) -> Vec<String>;

    /// Returns true if this check is disabled by default.
    fn default_off(&self) -> bool {
        true
    }

    fn mode(&self) -> Mode {
        Mode::Both
    }
}
