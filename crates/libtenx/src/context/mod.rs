/*!
Traits and implementations for including immutable reference material in model interactions. Each
context provider implements the `ContextProvider` trait and can generate one or more ContextItems
which are included in prompts.
*/

use enum_dispatch::enum_dispatch;

mod cmd;
mod manager;
mod path;
mod project_map;
mod ruskel;
mod text;
mod url;

pub use cmd::*;
pub use manager::*;
pub use path::*;
pub use project_map::*;
pub use ruskel::*;
pub use text::*;
pub use url::*;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{config::Config, error::Result, session::Session};

/// An individual context item.
#[derive(Debug, Serialize, Deserialize, Clone)]
/// Represents a single piece of context information to include in a prompt. Each ContextProvider
/// can provide multiple ContextItems.
pub struct ContextItem {
    /// The type of context.
    pub ty: String,
    /// The source of the context.
    pub source: String,
    /// The contents of the context.
    pub body: String,
}

// Custom implementation of PartialEq to match the semantics of is_dupe
impl PartialEq for Context {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Context {
    /// Returns true if both contexts have the same name and type.
    pub fn is_dupe(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

/// A trait for context providers that can be used to generate context items for a prompt.
#[async_trait]
#[enum_dispatch(Context)]
pub trait ContextProvider {
    /// Retrieves the context items for this provider.
    fn context_items(&self, config: &Config, session: &Session) -> Result<Vec<ContextItem>>;

    /// Returns a human-readable representation of the context provider.
    fn human(&self) -> String;

    /// Returns a unique identifier for this context provider.
    /// This ID is used for equality comparison between contexts.
    fn id(&self) -> String;

    /// Refreshes the content of the context provider.
    async fn refresh(&mut self, config: &Config) -> Result<()>;

    async fn needs_refresh(&self, _config: &Config) -> bool {
        false
    }
}

/// A context provider that produces reference material for model interactions.
#[enum_dispatch]
#[derive(Debug, Serialize, Deserialize, Clone, Eq)]
pub enum Context {
    /// API documentation generated using Ruskel
    Ruskel(Ruskel),
    /// One or more files matched by a path or glob pattern
    Path(Path),
    /// A list of all files in the project
    ProjectMap(ProjectMap),
    /// Content fetched from a remote URL
    Url(Url),
    /// Raw text content provided directly
    Text(Text),
    /// Output from executing a command
    Cmd(Cmd),
}

impl Context {
    /// Creates a new Context for plain text content.
    pub fn new_text(name: &str, content: &str) -> Self {
        Context::Text(Text::new(name.to_string(), content.to_string()))
    }

    /// Creates a new Context for a Ruskel document.
    pub fn new_ruskel(name: &str) -> Self {
        Context::Ruskel(Ruskel::new(name.to_string()))
    }

    /// Creates a new Context for a glob pattern.
    pub fn new_path(config: &Config, pattern: &str) -> Result<Self> {
        Ok(Context::Path(Path::new(config, pattern.to_string())?))
    }

    /// Creates a new Context for the project map.
    pub fn new_project_map() -> Self {
        Context::ProjectMap(ProjectMap::new())
    }

    /// Creates a new Context for a URL.
    pub fn new_url(url: &str) -> Self {
        Context::Url(Url::new(url.to_string()))
    }

    /// Creates a new Context for a command.
    pub fn new_cmd(command: &str) -> Self {
        Context::Cmd(Cmd::new(command.to_string()))
    }
}
