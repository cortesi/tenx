//! Traits and implementations for different styles of interaction with models.
use enum_dispatch::enum_dispatch;
use std::path::PathBuf;

mod dummy_dialect;
mod tags;
mod xmlish;

use crate::{
    config::Config,
    error::Result,
    session::{ModelResponse, Session},
};

pub use dummy_dialect::*;
pub use tags::*;

/// A dialect encapsulates a particular style of interaction with a model. It defines the system
/// prompt, how to render a user's prompt, and how to parse a model's response.
/// A trait defining the behavior of a dialect, including rendering and parsing capabilities.
#[enum_dispatch(Dialect)]
pub trait DialectProvider {
    /// Return the name of this dialect.
    fn name(&self) -> &'static str;

    /// Return the system prompt for this dialect.
    fn system(&self) -> String;

    /// Render the immutable context to be sent to the model. This is included once in the
    /// conversation.
    fn render_context(&self, config: &Config, p: &Session) -> Result<String>;

    /// Render the request portion of a step.
    fn render_step_request(
        &self,
        config: &Config,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
    ) -> Result<String>;

    /// Render the response portion of a step.
    fn render_step_response(
        &self,
        config: &Config,
        session: &Session,
        action_offset: usize,
        step_offset: usize,
    ) -> Result<String>;

    /// Render the editable context section
    fn render_editables(
        &self,
        config: &Config,
        session: &Session,
        paths: Vec<PathBuf>,
    ) -> Result<String>;

    /// Parse a model's response into concrete operations
    fn parse(&self, txt: &str) -> Result<ModelResponse>;
}

#[enum_dispatch]
#[derive(Debug, PartialEq, Eq, Clone)]
/// Represents different dialects for interacting with models.
pub enum Dialect {
    Tags(Tags),
    Dummy(DummyDialect),
}
