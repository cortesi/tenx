//! Traits and implementations for different styles of interaction with models.
use enum_dispatch::enum_dispatch;

#[cfg(test)]
mod tags_test;

mod tags;
mod xmlish;

use crate::{
    config::Config,
    error::{Result, TenxError},
    model::Chat,
    session::{ModelResponse, Session},
};

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

    /// Parse a model's response into concrete operations
    fn parse(&self, txt: &str) -> Result<ModelResponse>;

    fn build_chat(
        &self,
        _config: &Config,
        _session: &Session,
        _action_offset: usize,
        _chat: &mut Box<dyn Chat>,
    ) -> Result<()> {
        Err(TenxError::Internal(
            "build_chat not implemented".to_string(),
        ))
    }
}

#[enum_dispatch]
#[derive(Debug, PartialEq, Eq, Clone)]
/// Represents different dialects for interacting with models.
pub enum Dialect {
    Tags(Tags),
}
