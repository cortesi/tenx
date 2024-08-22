use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{prompt::PromptInput, Result, Session, SessionStore};

#[derive(Debug)]
pub struct Config {
    pub anthropic_key: String,
    pub session_store_dir: Option<PathBuf>,
    pub retry_limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_key: String::new(),
            session_store_dir: None,
            retry_limit: 3,
        }
    }
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

    /// Sets the retry limit.
    pub fn with_retry_limit(mut self, limit: usize) -> Self {
        self.retry_limit = limit;
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
    pub fn save_session(&self, session: &Session) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        session_store.save(session)?;
        Ok(())
    }

    /// Retries the last prompt in the session.
    pub async fn retry<P: AsRef<Path>>(
        &self,
        path: Option<P>,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let mut session = self.load_session(path)?;
        session.retry()?;
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        self.process_prompt(&mut session, sender, &session_store)
            .await
    }

    /// Loads a session from the store based on the working directory.
    pub fn load_session<P: AsRef<Path>>(&self, path: Option<P>) -> Result<Session> {
        let working_dir = crate::session::find_root(path);
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        session_store.load(working_dir)
    }

    /// Resumes a session by sending a prompt to the model.
    pub async fn resume(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
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
        session_store.save(session)?;
        self.run_preflight_validators(session)?;

        let mut retry_count = 0;
        loop {
            match self
                .execute_prompt_cycle(session, sender.clone(), session_store)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if let Some(model_message) = e.should_retry() {
                        retry_count += 1;
                        if retry_count >= self.config.retry_limit {
                            warn!("Retry limit reached. Last error: {}", e);
                            return Err(e);
                        }
                        warn!(
                            "Retryable error (attempt {}/{}): {}",
                            retry_count, self.config.retry_limit, e
                        );
                        session.add_prompt(PromptInput {
                            user_prompt: model_message.to_string(),
                        });
                    } else {
                        warn!("Non-retryable error: {}", e);
                        return Err(e);
                    }
                }
            }
        }
    }

    async fn execute_prompt_cycle(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<String>>,
        session_store: &SessionStore,
    ) -> Result<()> {
        let mut patch = session.prompt(&self.config, sender).await?;
        session.add_patch(&patch);
        session_store.save(session)?;
        session.apply_patch(&mut patch)?;
        session_store.save(session)?;
        self.run_post_patch_validators(session)?;
        Ok(())
    }

    fn run_preflight_validators(&self, session: &mut Session) -> Result<()> {
        let preflight_validators = crate::validators::preflight(session)?;
        for validator in preflight_validators {
            validator.validate(session)?;
        }
        Ok(())
    }

    fn run_post_patch_validators(&self, session: &mut Session) -> Result<()> {
        let post_patch_validators = crate::validators::post_patch(session)?;
        for validator in post_patch_validators {
            validator.validate(session)?;
        }
        Ok(())
    }
}

