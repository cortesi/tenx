use std::{env, path::Path};

use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{
    config::Config, context::ContextProvider, events::Event, prompt::Prompt,
    session_store::normalize_path, Result, Session, SessionStore, TenxError,
};

/// Tenx is an AI-driven coding assistant.
pub struct Tenx {
    pub config: Config,
}

impl Tenx {
    /// Creates a new Context with the specified configuration.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Creates a new Session, discovering the root from the current working directory and
    /// adding the default context from the config.
    pub fn session_from_cwd(&self) -> Result<Session> {
        let cwd = env::current_dir()
            .map_err(|e| TenxError::Internal(format!("Could not access cwd: {}", e)))?;
        let root = crate::config::find_project_root(&cwd);
        let mut session = Session::new(root);

        // Add default context
        self.add_ctx_ruskels(&mut session, &self.config.default_context.ruskel)?;
        self.add_ctx_globs(&mut session, &self.config.default_context.path)?;

        Ok(session)
    }

    /// Helper function to add multiple contexts and count items
    fn add_contexts_and_count(
        &self,
        session: &mut Session,
        contexts: Vec<crate::context::ContextSpec>,
    ) -> Result<usize> {
        let mut total_count = 0;
        for mut context in contexts {
            context.refresh()?;
            total_count += context.count(&self.config, session)?;
            session.add_context(context);
        }
        Ok(total_count)
    }

    /// Adds glob context to a session. Returns the total count of items added across all globs.
    pub fn add_ctx_globs(&self, session: &mut Session, ctx: &[String]) -> Result<usize> {
        let contexts = ctx
            .iter()
            .map(|file| crate::context::ContextSpec::new_glob(file.to_string()))
            .collect();
        self.add_contexts_and_count(session, contexts)
    }

    /// Adds ruskel context to a session. Returns the total count of items added.
    pub fn add_ctx_ruskels(&self, session: &mut Session, ruskel: &[String]) -> Result<usize> {
        let contexts = ruskel
            .iter()
            .map(|ruskel_doc| crate::context::ContextSpec::new_ruskel(ruskel_doc.clone()))
            .collect::<Vec<_>>();
        self.add_contexts_and_count(session, contexts)
    }

    /// Attempts to fix issues in the session by running preflight checks and adding a new prompt if there's an error.
    pub async fn fix(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let preflight_result = self.run_preflight_validators(session, &sender);
        if let Err(e) = preflight_result {
            session.add_prompt(Prompt::Auto(
                "Running the preflight checks to find errors to fix.".to_string(),
            ))?;
            session.set_last_error(&e);
            self.save_session(session)?;
        }
        Ok(())
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
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        session_store.save(session)?;
        Ok(())
    }

    /// Loads a session from the store based on the given path.
    pub fn load_session<P: AsRef<Path>>(&self, path: P) -> Result<Session> {
        let root = crate::config::find_project_root(path.as_ref());
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        let name = normalize_path(&root);
        session_store.load(name)
    }

    /// Loads a session from the store based on the current working directory.
    pub fn load_session_cwd(&self) -> Result<Session> {
        let current_dir = env::current_dir()
            .map_err(|e| TenxError::Internal(format!("Could not get cwd: {}", e)))?;
        let root = crate::config::find_project_root(&current_dir);
        self.load_session(root)
    }

    /// Retries the last prompt by rolling it back and sending it off for prompting..
    pub async fn retry(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        session.rollback_last()?;
        self.process_prompt(session, sender, &session_store).await
    }

    /// Sends a session off to the model for prompting.
    pub async fn prompt(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir())?;
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
        if session.last_step_error().is_none() {
            if let Err(e) = self.run_preflight_validators(session, &sender) {
                session.set_last_error(&e);
                session_store.save(session)?;
                return Err(e);
            }
        }

        let mut retry_count = 0;
        loop {
            // Pull out the next step generation, so that both fix and resuming a sesison with an
            // error works as expected.
            if let Some(e) = session.last_step_error() {
                if let Some(model_message) = e.should_retry() {
                    Self::send_event(&sender, Event::Retry(format!("{}", e)))?;
                    retry_count += 1;
                    if retry_count >= self.config.retry_limit {
                        warn!("Retry limit reached. Last error: {}", e);
                        return Err(e.clone());
                    }
                    debug!(
                        "Retryable error (attempt {}/{}): {}",
                        retry_count, self.config.retry_limit, e
                    );
                    session.add_prompt(Prompt::Auto(model_message.to_string()))?;
                    session_store.save(session)?;
                } else {
                    debug!("Non-retryable error: {}", e);
                    Self::send_event(&sender, Event::Fatal(format!("{}", e)))?;
                    return Err(e.clone());
                }
            }

            Self::send_event(&sender, Event::PromptStart)?;
            let result = self.execute_prompt_cycle(session, sender.clone()).await;
            Self::send_event(&sender, Event::PromptDone)?;
            match result {
                Ok(()) => {
                    session_store.save(session)?;
                    Self::send_event(&sender, Event::Finish)?;
                    return Ok(());
                }
                Err(e) => {
                    session.set_last_error(&e);
                }
            }
        }
    }

    async fn execute_prompt_cycle(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        session.prompt(&self.config, sender.clone()).await?;
        Self::send_event(&sender, Event::ApplyPatch)?;
        session.apply_last_patch()?;
        self.run_formatters(session, &sender)?;
        self.run_post_patch_validators(session, &sender)?;
        Ok(())
    }

    pub fn run_formatters(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        Self::send_event(sender, Event::FormattingStart)?;
        let formatters = crate::formatters::relevant_formatters(&self.config, session)?;
        for formatter in formatters {
            Self::send_event(sender, Event::FormatterStart(formatter.name().to_string()))?;
            formatter.format(session)?;
            Self::send_event(sender, Event::FormatterEnd(formatter.name().to_string()))?;
        }
        Self::send_event(sender, Event::FormattingEnd)?;
        Ok(())
    }

    pub fn run_preflight_validators(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        if self.config.no_preflight {
            return Ok(());
        }
        Self::send_event(sender, Event::PreflightStart)?;
        let preflight_validators = crate::validators::relevant_validators(&self.config, session)?;
        for validator in preflight_validators {
            Self::send_event(sender, Event::ValidatorStart(validator.name().to_string()))?;
            if let Err(e) = validator.validate(session) {
                Self::send_event(sender, Event::PreflightEnd)?;
                return Err(e);
            }
            Self::send_event(sender, Event::ValidatorOk(validator.name().to_string()))?;
        }
        Self::send_event(sender, Event::PreflightEnd)?;
        Ok(())
    }

    fn run_post_patch_validators(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        if let Some(last_step) = session.steps().last() {
            if last_step.patch.is_some() {
                Self::send_event(sender, Event::ValidationStart)?;
                let post_patch_validators =
                    crate::validators::relevant_validators(&self.config, session)?;
                for validator in post_patch_validators {
                    Self::send_event(sender, Event::ValidatorStart(validator.name().to_string()))?;
                    validator.validate(session)?;
                    Self::send_event(sender, Event::ValidatorOk(validator.name().to_string()))?;
                }
                Self::send_event(sender, Event::ValidationEnd)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fs_err as fs;
    use std::path::PathBuf;

    use crate::patch::{Change, Patch, WriteFile};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tenx_process_prompt() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let mut config =
            Config::default().with_dummy_model(crate::model::DummyModel::from_patch(Patch {
                changes: vec![Change::Write(WriteFile {
                    path: PathBuf::from("test.txt"),
                    content: "Updated content".to_string(),
                })],
                comment: Some("Test comment".to_string()),
                cache: Default::default(),
            }));
        config.session_store_dir = temp_dir.path().into();
        config.retry_limit = 1;

        let tenx = Tenx::new(config);
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "Initial content").unwrap();

        let mut session = Session::new(temp_dir.path().to_path_buf());
        session.add_prompt(Prompt::User("Test prompt".to_string()))?;
        session.add_editable_path(test_file_path.clone())?;

        let session_store = SessionStore::open(temp_dir.path().to_path_buf())?;
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
