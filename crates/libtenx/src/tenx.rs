use std::{
    env,
    path::{Path, PathBuf},
};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    events::Event, model, prompt::Prompt, session_store::normalize_path, Result, Session,
    SessionStore, TenxError,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub anthropic_key: String,
    pub session_store_dir: Option<PathBuf>,
    pub retry_limit: usize,
    pub no_preflight: bool,
    pub model: Option<model::Model>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_key: String::new(),
            session_store_dir: None,
            retry_limit: 10,
            no_preflight: false,
            model: None,
        }
    }
}

impl Config {
    /// Sets the Anthropic API key.
    pub fn with_anthropic_key(mut self, key: String) -> Self {
        self.anthropic_key = key;
        self
    }

    /// Sets the configured model
    pub fn with_model(mut self, model: model::Model) -> Self {
        self.model = Some(model);
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

    /// Sets the no_preflight flag.
    pub fn with_no_preflight(mut self, no_preflight: bool) -> Self {
        self.no_preflight = no_preflight;
        self
    }

    /// Returns the configured model.
    pub fn model(&self) -> Result<crate::model::Model> {
        self.model
            .clone()
            .ok_or_else(|| TenxError::Internal("Model not configured".to_string()))
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

    /// Helper function to send an event and handle potential errors.
    fn send_event(sender: &Option<mpsc::Sender<Event>>, event: Event) -> Result<()> {
        if let Some(sender) = sender {
            sender
                .try_send(event)
                .map_err(|e| TenxError::EventSend(e.to_string()))?;
        }
        Ok(())
    }

    /// Saves a session to the store.
    pub fn save_session(&self, session: &Session) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        session_store.save(session)?;
        Ok(())
    }

    /// Loads a session from the store based on the given path.
    pub fn load_session<P: AsRef<Path>>(&self, path: P) -> Result<Session> {
        let root = crate::session::find_root(path.as_ref());
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        let name = normalize_path(&root);
        session_store.load(name)
    }

    /// Loads a session from the store based on the current working directory.
    pub fn load_session_cwd(&self) -> Result<Session> {
        let current_dir = env::current_dir().map_err(|e| TenxError::fio(e, "."))?;
        let root = crate::session::find_root(&current_dir);
        self.load_session(root)
    }

    /// Resumes a session by sending a prompt to the model. If the last step has changes, they are
    /// rolled back.
    pub async fn resume(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        session.rollback_last()?;
        self.process_prompt(session, sender, &session_store).await
    }

    /// Resets the session to a specific step.
    pub fn reset(&self, session: &mut Session, offset: usize) -> Result<()> {
        session.reset(offset)?;
        self.save_session(session)
    }

    /// Common logic for processing a prompt and updating the state. The prompt that will be
    /// processed is the final prompt in the step list.
    async fn process_prompt(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
        session_store: &SessionStore,
    ) -> Result<()> {
        session_store.save(session)?;
        if let Err(e) = self.run_preflight_validators(session, &sender) {
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
                        session.add_prompt(Prompt::Auto(model_message.to_string()))?;
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
        sender: Option<mpsc::Sender<Event>>,
        session_store: &SessionStore,
    ) -> Result<()> {
        if let Err(e) = session.prompt(&self.config, sender.clone()).await {
            session.set_last_error(&e);
            return Err(e);
        }
        session_store.save(session)?;
        if let Err(e) = session.apply_last_patch() {
            session.set_last_error(&e);
            return Err(e);
        }
        session_store.save(session)?;
        if let Err(e) = self.run_formatters(session, &sender) {
            session.set_last_error(&e);
            return Err(e);
        }
        session_store.save(session)?;
        if let Err(e) = self.run_post_patch_validators(session, &sender) {
            session.set_last_error(&e);
            return Err(e);
        }
        Ok(())
    }

    fn run_formatters(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        Self::send_event(sender, Event::FormattingStart)?;
        let formatters = crate::formatters::formatters(session)?;
        for formatter in formatters {
            formatter.format(session)?;
            Self::send_event(sender, Event::FormattingOk(formatter.name().to_string()))?;
        }
        Self::send_event(sender, Event::FormattingEnd)?;
        Ok(())
    }

    fn run_preflight_validators(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        if self.config.no_preflight {
            return Ok(());
        }
        Self::send_event(sender, Event::PreflightStart)?;
        let preflight_validators = crate::validators::preflight(session)?;
        for validator in preflight_validators {
            if let Err(e) = validator.validate(session) {
                if let TenxError::Validation { name, user, model } = e {
                    return Err(TenxError::Preflight { name, user, model });
                }
                return Err(e);
            }
        }
        Self::send_event(sender, Event::PreflightEnd)?;
        Ok(())
    }

    fn run_post_patch_validators(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        Self::send_event(sender, Event::ValidationStart)?;
        let post_patch_validators = crate::validators::post_patch(session)?;
        for validator in post_patch_validators {
            validator.validate(session)?;
        }
        Self::send_event(sender, Event::ValidationEnd)?;
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
            .with_model(Model::Dummy(crate::model::DummyModel::from_patch(Patch {
                changes: vec![Change::Write(WriteFile {
                    path: PathBuf::from("test.txt"),
                    content: "Updated content".to_string(),
                })],
                comment: Some("Test comment".to_string()),
                cache: Default::default(),
            })))
            .with_session_store_dir(temp_dir.path())
            .with_retry_limit(1);
        let tenx = Tenx::new(config);
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "Initial content").unwrap();

        let mut session = Session::new(
            temp_dir.path().to_path_buf(),
            Dialect::Dummy(DummyDialect::default()),
        );
        session.add_prompt(Prompt::User("Test prompt".to_string()))?;
        session.add_editable(test_file_path.clone())?;

        let session_store = SessionStore::open(Some(temp_dir.path().to_path_buf()))?;
        tenx.process_prompt(&mut session, None, &session_store)
            .await?;

        assert_eq!(session.steps().len(), 1);
        assert!(session.steps()[0].patch.is_some());
        assert_eq!(
            session.steps()[0].patch.as_ref().unwrap().comment,
            Some("Test comment".to_string())
        );

        let file_content = fs::read_to_string(&test_file_path).unwrap();
        assert_eq!(file_content, "Updated content");

        Ok(())
    }
}
