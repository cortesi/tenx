use serde::{Deserialize, Serialize};

mod tags;

use crate::{Operations, PromptInput, Result};

pub use tags::*;

/// A dialect encapsulates a particular style of interaction with a model. It defines the system
/// prompt, how to render a user's prompt, and how to parse a model's response.
pub trait DialectProvider {
    /// Return the system prompt for this dialect
    fn system(&self) -> String;
    /// Render a prompt to send to the model
    fn render(&self, p: &PromptInput) -> Result<String>;
    /// Parse a model's response into concrete operations
    fn parse(&self, txt: &str) -> Result<Operations>;
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

    fn render(&self, p: &PromptInput) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render(p),
        }
    }

    fn parse(&self, txt: &str) -> Result<Operations> {
        match self {
            Dialect::Tags(t) => t.parse(txt),
        }
    }
}
