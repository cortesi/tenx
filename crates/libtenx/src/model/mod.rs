use serde::{Deserialize, Serialize};

mod claude;
mod dummy_model;

pub use claude::Claude;
pub use dummy_model::DummyModel;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{events::Event, patch::Patch, Config, Result, Session};

/// Implemented by types that expose a prompt operation.
#[async_trait]
pub trait ModelProvider {
    /// Returns the name of the model provider.
    fn name(&self) -> &'static str;

    /// Render and send a session to the model.
    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<Patch>;

    /// Render a session for display to the user.
    fn render(&self, session: &Session) -> Result<String>;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Model {
    Claude(Claude),
    Dummy(DummyModel),
}

#[async_trait]
impl ModelProvider for Model {
    fn name(&self) -> &'static str {
        match self {
            Model::Claude(c) => c.name(),
            Model::Dummy(d) => d.name(),
        }
    }

    async fn send(
        &mut self,
        config: &Config,
        session: &Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<Patch> {
        match self {
            Model::Claude(c) => c.send(config, session, sender).await,
            Model::Dummy(d) => d.send(config, session, sender).await,
        }
    }

    fn render(&self, session: &Session) -> Result<String> {
        match self {
            Model::Claude(c) => c.render(session),
            Model::Dummy(d) => d.render(session),
        }
    }
}
