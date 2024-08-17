use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod tags;
mod xmlish;

use crate::{Patch, PromptInput, Result, Session};

pub use tags::*;

/// A dialect encapsulates a particular style of interaction with a model. It defines the system
/// prompt, how to render a user's prompt, and how to parse a model's response.
pub trait DialectProvider {
    /// Return the system prompt for this dialect
    fn system(&self) -> String;

    /// Render a prompt to send to the model
    fn render_prompt(&self, p: &PromptInput) -> Result<String>;

    /// Render the editable context section
    fn render_editables(&self, paths: Vec<PathBuf>) -> Result<String>;

    /// Render the immutable context to be sent to the model
    fn render_context(&self, p: &Session) -> Result<String>;

    /// Parse a model's response into concrete operations
    fn parse(&self, txt: &str) -> Result<Patch>;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Dialect {
    Tags(Tags),
}

impl DialectProvider for Dialect {
    fn system(&self) -> String {
        match self {
            Dialect::Tags(t) => t.system(),
        }
    }

    fn render_context(&self, s: &Session) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_context(s),
        }
    }

    fn render_editables(&self, paths: Vec<PathBuf>) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_editables(paths),
        }
    }

    fn render_prompt(&self, p: &PromptInput) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_prompt(p),
        }
    }

    fn parse(&self, txt: &str) -> Result<Patch> {
        match self {
            Dialect::Tags(t) => t.parse(txt),
        }
    }
}
