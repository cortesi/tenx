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
            retry_limit: 7,
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
        if let Err(e) = self.run_preflight_validators(session) {
            session.set_last_error(&e);
            return Err(e);
        }

        let mut retry_count = 0;
        loop {
            match self
                .execute_prompt_cycle(session, sender.clone(), session_store)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    session.set_last_error(&e);
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
                        })?;
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
        let mut patch = match session.prompt(&self.config, sender).await {
            Ok(patch) => patch,
            Err(e) => {
                session.set_last_error(&e);
                return Err(e);
            }
        };
        session.set_last_patch(&patch);
        session_store.save(session)?;
        if let Err(e) = session.apply_patch(&mut patch) {
            session.set_last_error(&e);
            return Err(e);
        }
        session_store.save(session)?;
        if let Err(e) = self.run_post_patch_validators(session) {
            session.set_last_error(&e);
            return Err(e);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dialect::{Dialect, DummyDialect},
        model::Model,
        patch::{Change, Patch, WriteFile},
    };
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tenx_process_prompt() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let config = Config::default()
            .with_session_store_dir(temp_dir.path())
            .with_retry_limit(1);
        let tenx = Tenx::new(config);
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "Initial content").unwrap();

        let mut session = Session::new(
            Some(temp_dir.path().to_path_buf()),
            Dialect::Dummy(DummyDialect::default()),
            Model::Dummy(crate::model::DummyModel::from_patch(Patch {
                changes: vec![Change::Write(WriteFile {
                    path: PathBuf::from("test.txt"),
                    content: "Updated content".to_string(),
                })],
                comment: Some("Test comment".to_string()),
                cache: Default::default(),
            })),
        );
        session.add_prompt(PromptInput {
            user_prompt: "Test prompt".to_string(),
        })?;
        session.add_editable(test_file_path.clone())?;

        let session_store = SessionStore::open(Some(temp_dir.path().to_path_buf()))?;
        tenx.process_prompt(&mut session, None, &session_store)
            .await?;

        assert_eq!(session.steps.len(), 1);
        assert!(session.steps[0].patch.is_some());
        assert_eq!(
            session.steps[0].patch.as_ref().unwrap().comment,
            Some("Test comment".to_string())
        );

        let file_content = fs::read_to_string(&test_file_path).unwrap();
        assert_eq!(file_content, "Updated content");

        Ok(())
    }
}
