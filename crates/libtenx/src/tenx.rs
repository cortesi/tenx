use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{
    config::Config,
    context::{Context, ContextProvider},
    events::*,
    prompt::Prompt,
    session_store::normalize_path,
    Result, Session, SessionStore, TenxError,
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
    pub fn new_session_from_cwd(&self, sender: &Option<mpsc::Sender<Event>>) -> Result<Session> {
        let mut session = Session::default();

        // Add default context
        self.add_contexts(
            &mut session,
            &self.config.default_context.path,
            &self.config.default_context.ruskel,
            sender,
        )?;

        // Add project map if configured
        if self.config.default_context.project_map {
            let context = Context::new_project_map();
            send_event(sender, Event::ContextStart)?;
            session.add_context(context);
            send_event(sender, Event::ContextEnd)?;
        }

        send_event(sender, Event::Finish)?;
        Ok(session)
    }

    /// Adds contexts to a session in a batch. Returns the total count of items added.
    pub fn add_contexts(
        &self,
        session: &mut Session,
        glob: &[String],
        ruskel: &[String],
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<usize> {
        let mut contexts = Vec::new();
        for file in glob {
            contexts.push(Context::new_path(&self.config, file.to_string())?);
        }
        for ruskel_doc in ruskel {
            contexts.push(Context::new_ruskel(ruskel_doc.clone()));
        }
        let mut total_added = 0;
        if !contexts.is_empty() {
            send_event(sender, Event::ContextStart)?;
            for mut context in contexts {
                send_event(
                    sender,
                    Event::ContextRefreshStart(context.name().to_string()),
                )?;
                context.refresh()?;
                send_event(sender, Event::ContextRefreshEnd(context.name().to_string()))?;
                total_added += context.count(&self.config, session)?;
                session.add_context(context);
            }
            send_event(sender, Event::ContextEnd)?;
        }

        send_event(sender, Event::Finish)?;
        Ok(total_added)
    }

    /// Refreshes all contexts in the specified session.
    pub fn refresh_context(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        send_event(sender, Event::ContextStart)?;
        {
            for context in session.contexts.iter_mut() {
                send_event(
                    sender,
                    Event::ContextRefreshStart(context.name().to_string()),
                )?;
                context.refresh()?;
                send_event(sender, Event::ContextRefreshEnd(context.name().to_string()))?;
            }
        }
        send_event(sender, Event::ContextEnd)?;
        Ok(())
    }

    /// Attempts to fix issues in the session by running preflight checks and adding a new prompt if there's an error.
    pub async fn fix(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
        prompt: Option<String>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        let preflight_result = self.run_preflight_validators(session, &sender);
        let result = if let Err(e) = preflight_result {
            let prompt = prompt.unwrap_or_else(|| "Please fix the following errors.".to_string());
            session.add_prompt(Prompt::Auto(prompt))?;
            if let Some(step) = session.last_step_mut() {
                step.err = Some(e.clone());
            }
            self.save_session(session)?;
            self.process_prompt(session, sender.clone(), &session_store)
                .await
        } else {
            Err(TenxError::Internal("No errors found".to_string()))
        };
        send_event(&sender, Event::Finish)?;
        result
    }

    /// Saves a session to the store.
    pub fn save_session(&self, session: &Session) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        session_store.save(&self.config, session)?;
        Ok(())
    }

    /// Loads a session from the store.
    pub fn load_session(&self) -> Result<Session> {
        let root = self.config.project_root();
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        let name = normalize_path(&root);
        session_store.load(name)
    }

    /// Retries the last prompt, optionally replacing it with a new one.
    pub async fn retry(
        &self,
        session: &mut Session,
        prompt: Option<String>,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        if let Some(step) = session.last_step_mut() {
            step.rollback(&self.config)?;
            if let Some(p) = prompt {
                step.prompt = Prompt::User(p);
            }
        }
        let result = self
            .process_prompt(session, sender.clone(), &session_store)
            .await;
        send_event(&sender, Event::Finish)?;
        result
    }

    /// Adds a user prompt to the session and sends it to the model.
    pub async fn ask(
        &self,
        session: &mut Session,
        prompt: String,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        session.add_prompt(Prompt::User(prompt))?;
        let session_store = SessionStore::open(self.config.session_store_dir())?;
        let result = self
            .process_prompt(session, sender.clone(), &session_store)
            .await;
        send_event(&sender, Event::Finish)?;
        result
    }

    /// Resets the session to a specific step.
    pub fn reset(&self, session: &mut Session, offset: usize) -> Result<()> {
        session.reset(&self.config, offset)?;
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
        session_store.save(&self.config, session)?;
        if session.last_step_error().is_none() {
            if let Err(e) = self.run_preflight_validators(session, &sender) {
                if let Some(step) = session.last_step_mut() {
                    step.err = Some(e.clone());
                }
                session_store.save(&self.config, session)?;
                return Err(e);
            }
        }

        let mut retry_count = 0;
        loop {
            if let Some(e) = session.last_step_error() {
                if let Some(model_message) = e.should_retry() {
                    send_event(
                        &sender,
                        Event::Retry {
                            user: format!("{}", e),
                            model: model_message.to_string(),
                        },
                    )?;
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
                    session_store.save(&self.config, session)?;
                } else {
                    debug!("Non-retryable error: {}", e);
                    send_event(&sender, Event::Fatal(format!("{}", e)))?;
                    return Err(e.clone());
                }
            }

            send_event(&sender, Event::PromptStart)?;
            let result = self.execute_prompt_cycle(session, sender.clone()).await;
            send_event(&sender, Event::PromptEnd)?;
            match result {
                Ok(()) => {
                    session_store.save(&self.config, session)?;
                    send_event(&sender, Event::Finish)?;
                    if !session.should_continue() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    if let Some(step) = session.last_step_mut() {
                        step.err = Some(e.clone());
                    }
                }
            }
        }
    }

    async fn execute_prompt_cycle(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        send_event(&sender, Event::ModelRequestStart)?;
        let prompt_result = session.prompt(&self.config, sender.clone()).await;
        send_event(&sender, Event::ModelRequestEnd)?;
        prompt_result?;
        send_event(&sender, Event::ApplyPatch)?;
        session.apply_last_step(&self.config)?;
        if !session.should_continue() {
            // We're done, now we check if formatter or validators return an error we need to
            // process
            self.run_formatters(session, &sender)?;
            self.run_post_patch_validators(session, &sender)?;
        }
        Ok(())
    }

    pub fn run_formatters(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        send_event(sender, Event::FormattingStart)?;
        let formatters = crate::formatters::relevant_formatters(&self.config, session)?;
        for formatter in formatters {
            send_event(sender, Event::FormatterStart(formatter.name().to_string()))?;
            formatter.format(&self.config, session)?;
            send_event(sender, Event::FormatterEnd(formatter.name().to_string()))?;
        }
        send_event(sender, Event::FormattingEnd)?;
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
        send_event(sender, Event::PreflightStart)?;
        let preflight_validators = crate::validators::relevant_validators(&self.config, session)?;
        for validator in preflight_validators {
            send_event(sender, Event::ValidatorStart(validator.name().to_string()))?;
            if let Err(e) = validator.validate(&self.config, session) {
                send_event(sender, Event::PreflightEnd)?;
                return Err(e);
            }
            send_event(sender, Event::ValidatorOk(validator.name().to_string()))?;
        }
        send_event(sender, Event::PreflightEnd)?;
        Ok(())
    }

    fn run_post_patch_validators(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        if let Some(last_step) = session.steps().last() {
            if last_step.model_response.is_some() {
                send_event(sender, Event::PostPatchStart)?;
                let post_patch_validators =
                    crate::validators::relevant_validators(&self.config, session)?;
                for validator in post_patch_validators {
                    send_event(sender, Event::ValidatorStart(validator.name().to_string()))?;
                    validator.validate(&self.config, session)?;
                    send_event(sender, Event::ValidatorOk(validator.name().to_string()))?;
                }
                send_event(sender, Event::PostPatchEnd)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::patch::{Change, Patch, WriteFile};
    use crate::ModelResponse;

    use fs_err as fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tenx_process_prompt() {
        let temp_dir = tempdir().unwrap();
        let mut config = Config::default()
            .with_dummy_model(crate::model::DummyModel::from_model_response(
                ModelResponse {
                    comment: Some("Test comment".to_string()),
                    patch: Some(Patch {
                        changes: vec![Change::Write(WriteFile {
                            path: PathBuf::from("test.txt"),
                            content: "Updated content".to_string(),
                        })],
                    }),
                    operations: vec![],
                    usage: None,
                },
            ))
            .with_root(temp_dir.path());
        config.session_store_dir = temp_dir.path().join("sess");
        config.retry_limit = 1;

        let tenx = Tenx::new(config.clone());
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "Initial content").unwrap();

        let mut session = Session::default();
        session
            .add_prompt(Prompt::User("Test prompt".to_string()))
            .unwrap();
        session
            .add_editable_path(&config, test_file_path.clone())
            .unwrap();

        let session_store = SessionStore::open(config.session_store_dir).unwrap();
        tenx.process_prompt(&mut session, None, &session_store)
            .await
            .unwrap();

        assert_eq!(session.steps().len(), 1);
        assert!(session.steps()[0].model_response.is_some());
        assert_eq!(
            session.steps()[0].model_response.as_ref().unwrap().comment,
            Some("Test comment".to_string())
        );

        let file_content = fs::read_to_string(&test_file_path).unwrap();
        assert_eq!(file_content, "Updated content");
    }
}
