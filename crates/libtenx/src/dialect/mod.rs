mod tags;

use crate::{Context, Operations, Prompt, Result};

pub use tags::*;

/// A dialect encapsulates a particular style of interaction with a model. It defines the system
/// prompt, how to render a user's prompt, and how to parse a model's response.
pub trait Dialect {
    /// Return the system prompt for this model
    fn system(&self) -> String;
    /// Render a prompt to send to the model
    fn render(&self, ctx: Context, p: &Prompt) -> Result<String>;
    /// Parse a model's response into concrete operations
    fn parse(&self, txt: &str) -> Result<Operations>;
}