//! Traits and implementations for different styles of interaction with models.
use std::path::PathBuf;

mod dummy_dialect;
mod tags;
mod xmlish;

use crate::{
    config::Config,
    session::{ModelResponse, Session},
    Result,
};

pub use dummy_dialect::*;
pub use tags::*;

/// A dialect encapsulates a particular style of interaction with a model. It defines the system
/// prompt, how to render a user's prompt, and how to parse a model's response.
/// A trait defining the behavior of a dialect, including rendering and parsing capabilities.
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
        offset: usize,
    ) -> Result<String>;

    /// Render the response portion of a step.
    fn render_step_response(
        &self,
        config: &Config,
        session: &Session,
        offset: usize,
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

#[derive(Debug, PartialEq, Eq, Clone)]
/// Represents different dialects for interacting with models.
pub enum Dialect {
    Tags(Tags),
    Dummy(DummyDialect),
}

impl DialectProvider for Dialect {
    /// Return the name of this dialect.
    fn name(&self) -> &'static str {
        match self {
            Dialect::Tags(t) => t.name(),
            Dialect::Dummy(d) => d.name(),
        }
    }

    /// Return the system prompt for this dialect.
    fn system(&self) -> String {
        match self {
            Dialect::Tags(t) => t.system(),
            Dialect::Dummy(d) => d.system(),
        }
    }

    /// Render the immutable context to be sent to the model.
    fn render_context(&self, config: &Config, s: &Session) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_context(config, s),
            Dialect::Dummy(d) => d.render_context(config, s),
        }
    }

    fn render_editables(
        &self,
        config: &Config,
        session: &Session,
        paths: Vec<PathBuf>,
    ) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_editables(config, session, paths),
            Dialect::Dummy(d) => d.render_editables(config, session, paths),
        }
    }

    fn render_step_request(
        &self,
        config: &Config,
        session: &Session,
        offset: usize,
    ) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_step_request(config, session, offset),
            Dialect::Dummy(d) => d.render_step_request(config, session, offset),
        }
    }

    /// Parse a model's response into concrete operations.
    fn parse(&self, txt: &str) -> Result<ModelResponse> {
        match self {
            Dialect::Tags(t) => t.parse(txt),
            Dialect::Dummy(d) => d.parse(txt),
        }
    }

    fn render_step_response(
        &self,
        config: &Config,
        session: &Session,
        offset: usize,
    ) -> Result<String> {
        match self {
            Dialect::Tags(t) => t.render_step_response(config, session, offset),
            Dialect::Dummy(d) => d.render_step_response(config, session, offset),
        }
    }
}
