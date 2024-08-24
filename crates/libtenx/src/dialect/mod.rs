use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod dummy_dialect;
mod tags;
mod xmlish;

use crate::{patch::Patch, prompt::PromptInput, Result, Session};

pub use dummy_dialect::*;
pub use tags::*;

/// A dialect encapsulates a particular style of interaction with a model. It defines the system
/// prompt, how to render a user's prompt, and how to parse a model's response.
pub trait DialectProvider {
    /// Return the name of this dialect
    fn name(&self) -> &'static str;

    /// Return the system prompt for this dialect
    fn system(&self) -> String;

    /// Render the request portion of a step.
    fn render_step_request(&self, session: &Session, offset: usize) -> Result<String>;

    /// Render the editable context section
    fn render_editables(&self, paths: Vec<PathBuf>) -> Result<String>;

    /// Render the immutable context to be sent to the model. This is included once in the
    /// conversation.
    fn render_context(&self, p: &Session) -> Result<String>;

    /// Render a Patch into a string representation
    fn render_patch(&self, patch: &Patch) -> Result<String>;

    /// Parse a model's response into concrete operations
    fn parse(&self, txt: &str) -> Result<Patch>;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum Dialect {
    Tags(Tags),
    Dummy(DummyDialect),
}

impl DialectProvider for Dialect {
    fn name(&self) -> &'static str {
        match self {
            Dialect::Tags(t) => t.name(),
            Dialect::Dummy(d) => d.name(),
        }
    }

    fn system(&self) -> String {
        match self {
            Dialect::Tags(t) => t.system(),
            Dialect::Dummy(d) => d.system(),
        }
    }

    fn render_context(&self, s: &Session) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_context(s),
            Dialect::Dummy(d) => d.render_context(s),
        }
    }

    fn render_editables(&self, paths: Vec<PathBuf>) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_editables(paths),
            Dialect::Dummy(d) => d.render_editables(paths),
        }
    }

    fn render_step_request(&self, session: &Session, offset: usize) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_step_request(session, offset),
            Dialect::Dummy(d) => d.render_step_request(session, offset),
        }
    }

    fn parse(&self, txt: &str) -> Result<Patch> {
        match self {
            Dialect::Tags(t) => t.parse(txt),
            Dialect::Dummy(d) => d.parse(txt),
        }
    }

    fn render_patch(&self, patch: &Patch) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_patch(patch),
            Dialect::Dummy(d) => d.render_patch(patch),
        }
    }
}

