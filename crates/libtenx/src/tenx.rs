use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{model::ModelProvider, Result, Session, SessionStore};

#[derive(Debug, Default)]
pub struct Config {
    pub anthropic_key: String,
    pub session_store_dir: Option<PathBuf>,
}

impl Config {
    /// Sets the Anthropic API key.
    pub fn with_anthropic_key(mut self, key: String) -> Self {
        self.anthropic_key = key;
        self
    }

    /// Sets the state directory.
    pub fn with_session_store_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.session_store_dir = Some(dir.as_ref().to_path_buf());
        self
    }
}

/// Tenx is an AI-driven coding assistant.
pub struct Tenx {
    pub config: Config,
}

impl Tenx {
    /// Creates a new Context with the specified configuration.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Saves a session to the store.
    pub fn save_session(&self, session: Session) -> Result<Session> {
        let session_store = SessionStore::open(self.config.session_store_dir.as_ref())?;
        session_store.save(&session)?;
        Ok(session)
    }

    /// Loads a session from the store based on the working directory.
    pub fn load_session<P: AsRef<Path>>(&self, path: Option<P>) -> Result<Session> {
        let working_dir = crate::session::find_working_dir(path);
        let session_store = SessionStore::open(self.config.session_store_dir.as_ref())?;
        session_store.load(working_dir)
    }

    /// Resets all files in the state snapshot to their original contents.
    pub fn reset(_state: &Session) -> Result<()> {
        // FIXME
        Ok(())
    }

    /// Resumes a session by sending a prompt to the model.
    pub async fn resume(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir.as_ref())?;
        self.process_prompt(session, sender, &session_store).await
    }

    /// Common logic for processing a prompt and updating the state. The prompt that will be
    /// processed is the final prompt in the step list.
    async fn process_prompt(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<String>>,
        session_store: &SessionStore,
    ) -> Result<()> {
        let mut model = session.model.take().unwrap();
        let patch = model.prompt(&self.config, session, sender).await?;
        session.model = Some(model);
        match session.apply_patch(&patch) {
            Ok(_) => {
                session_store.save(session)?;
                Ok(())
            }
            Err(e) => {
                warn!("{}", e);
                warn!("Resetting state...");
                Self::reset(session)?;
                Err(e)
            }
        }
    }
}

