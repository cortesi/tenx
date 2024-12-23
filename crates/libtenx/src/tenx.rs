use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{
    checks::CheckMode,
    config::Config,
    context::{Context, ContextProvider},
    events::{send_event, Event, EventBlock},
    model::ModelProvider,
    session::{Session, StepType},
    session_store::{path_to_filename, SessionStore},
    Result, TenxError,
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
    pub async fn new_session_from_cwd(
        &self,
        sender: &Option<mpsc::Sender<Event>>,
        no_context: bool,
    ) -> Result<Session> {
        let _block = EventBlock::start(sender)?;
        let mut session = Session::default();

        if !no_context {
            // Add path contexts
            for path in &self.config.context.path {
                session.add_context(Context::new_path(&self.config, path)?);
            }

            // Add ruskel contexts
            for ruskel in &self.config.context.ruskel {
                session.add_context(Context::new_ruskel(ruskel));
            }

            // Add text contexts
            for text in &self.config.context.text {
                session.add_context(Context::new_text(&text.name, &text.content));
            }

            // Add project map if configured
            if self.config.context.project_map {
                session.add_context(Context::new_project_map());
            }
        }

        // Refresh all contexts
        self.refresh_contexts_inner(&mut session, sender).await?;
        Ok(session)
    }

    /// Refreshes all contexts in the session, but don't create a new event block.
    async fn refresh_contexts_inner(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        if session.contexts.is_empty() {
            return Ok(());
        }

        let _block = EventBlock::context(sender)?;
        for context in session.contexts.iter_mut() {
            let _refresh_block = EventBlock::context_refresh(sender, &context.human())?;
            context.refresh(&self.config).await?;
        }
        Ok(())
    }

    /// Refreshes all contexts in the session.
    pub async fn refresh_contexts(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let _block = EventBlock::start(sender)?;
        self.refresh_contexts_inner(session, sender).await
    }

    /// Refreshes only contexts that need refreshing according to their needs_refresh() method.
    pub async fn refresh_needed_contexts(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let _block = EventBlock::start(sender)?;
        if !session.contexts.is_empty() {
            let _block = EventBlock::context(sender)?;
            for context in session.contexts.iter_mut() {
                if context.needs_refresh(&self.config).await {
                    let _refresh_block = EventBlock::context_refresh(sender, &context.human())?;
                    context.refresh(&self.config).await?;
                }
            }
        }
        Ok(())
    }

    /// Attempts to fix issues in the session by running pre checks and adding a new prompt if there's an error.
    pub async fn fix(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
        prompt: Option<String>,
    ) -> Result<()> {
        let _block = EventBlock::start(&sender)?;
        let pre_result = self.run_pre_checks(session, &sender);
        let result = if let Err(e) = pre_result {
            let prompt = prompt.unwrap_or_else(|| "Please fix the following errors.".to_string());
            let model = self.config.models.default.clone();
            session.add_prompt(model, prompt, StepType::Fix)?;
            if let Some(step) = session.last_step_mut() {
                step.err = Some(e.clone());
            }
            self.save_session(session)?;
            self.process_prompt(session, sender.clone()).await
        } else {
            Err(TenxError::Internal("No errors found".to_string()))
        };
        result
    }

    /// Saves a session to the store.
    pub fn save_session(&self, session: &Session) -> Result<()> {
        if self.config.session_store_dir.as_os_str().is_empty() {
            return Ok(());
        }
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        let root = self.config.project_root();
        let name = path_to_filename(&root);
        session_store.save(&name, session)
    }

    /// Loads a session from the store.
    pub fn load_session(&self) -> Result<Session> {
        let root = self.config.project_root();
        let session_store = SessionStore::open(self.config.session_store_dir.clone())?;
        let name = path_to_filename(&root);
        session_store.load(name)
    }

    /// Retries the last prompt, optionally replacing it with a new one.
    pub async fn retry(
        &self,
        session: &mut Session,
        prompt: Option<String>,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let _block = EventBlock::start(&sender)?;
        if let Some(step) = session.last_step_mut() {
            step.rollback(&self.config)?;
            if let Some(p) = prompt {
                step.prompt = p;
                step.step_type = StepType::Code;
            }
        }
        self.process_prompt(session, sender.clone()).await
    }

    /// Adds a user prompt to the session and sends it to the model.
    pub async fn code(
        &self,
        session: &mut Session,
        prompt: String,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let _block = EventBlock::start(&sender)?;
        let model = self.config.models.default.clone();
        session.add_prompt(model, prompt, StepType::Code)?;
        self.process_prompt(session, sender.clone()).await
    }

    /// Resets the session to a specific step.
    pub fn reset(&self, session: &mut Session, offset: usize) -> Result<()> {
        session.reset(&self.config, offset)?;
        self.save_session(session)
    }

    /// Resets all steps in the session.
    pub fn reset_all(&self, session: &mut Session) -> Result<()> {
        session.reset_all(&self.config)?;
        self.save_session(session)
    }

    pub fn check(&self, paths: Vec<PathBuf>, sender: &Option<mpsc::Sender<Event>>) -> Result<()> {
        let _block = EventBlock::start(sender)?;
        self.check_paths(&paths, CheckMode::Both, sender)
    }

    /// Run checks based on an optional session and an optional mode filter.
    fn run_checks(
        &self,
        session: &Session,
        mode_filter: CheckMode,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let paths = if session.editables().is_empty() {
            self.config.project_files()?
        } else {
            session.editables().to_vec()
        };

        self.check_paths(
            &paths.iter().map(PathBuf::from).collect(),
            mode_filter,
            sender,
        )
    }

    /// Run checks on a given set of paths with a mode filter.
    fn check_paths(
        &self,
        paths: &Vec<PathBuf>,
        mode_filter: CheckMode,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        for c in self.config.enabled_checks() {
            let is_mode_match = c.mode == mode_filter || c.mode == CheckMode::Both;
            if is_mode_match && c.is_relevant(paths)? {
                let _check_block = EventBlock::validator(sender, &c.name)?;
                c.check(&self.config)?;
            }
        }
        Ok(())
    }

    /// Common logic for processing a prompt and updating the state. The prompt that will be
    /// processed is the final prompt in the step list.
    async fn process_prompt(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        self.save_session(session)?;
        if session.last_step_error().is_none() {
            if let Err(e) = self.run_pre_checks(session, &sender) {
                if let Some(step) = session.last_step_mut() {
                    step.err = Some(e.clone());
                }
                self.save_session(session)?;
                return Err(e);
            }
        }

        let mut retry_count = 0;
        loop {
            if let Some(e) = session.last_step_error() {
                if let Some(model_message) = e.should_retry() {
                    if retry_count >= self.config.retry_limit {
                        warn!("Retry limit reached. Last error: {}", e);
                        send_event(
                            &sender,
                            Event::Fatal(format!("Retry limit reached. Last error: {}", e)),
                        )?;
                        return Err(e.clone());
                    }
                    send_event(
                        &sender,
                        Event::Retry {
                            user: format!("{}", e),
                            model: model_message.to_string(),
                        },
                    )?;
                    retry_count += 1;
                    debug!(
                        "Retryable error (attempt {}/{}): {}",
                        retry_count, self.config.retry_limit, e
                    );
                    let model = self.config.models.default.clone();
                    session.add_prompt(model, model_message.to_string(), StepType::Error)?;
                    self.save_session(session)?;
                } else {
                    debug!("Non-retryable error: {}", e);
                    send_event(&sender, Event::Fatal(format!("{}", e)))?;
                    return Err(e.clone());
                }
            }

            let result = self.execute_prompt_cycle(session, sender.clone()).await;
            match result {
                Ok(()) => {
                    self.save_session(session)?;
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
        self.prompt(session, sender.clone()).await?;
        send_event(&sender, Event::ApplyPatch)?;
        session.apply_last_step(&self.config)?;
        if !session.should_continue() {
            // We're done, now we check if checks return an error we need to process
            self.run_post_checks(session, &sender)?;
        }
        Ok(())
    }

    /// Prompts the current model with the session's state and sets the resulting patch and usage.
    async fn prompt(
        &self,
        session: &mut Session,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let mut model = self.config.active_model()?;
        let _block = EventBlock::prompt(&sender, &model.name())?;
        let start_time = std::time::Instant::now();
        let resp = model.send(&self.config, session, sender).await?;
        let elapsed = start_time.elapsed().as_secs_f64();
        if let Some(last_step) = session.last_step_mut() {
            last_step.model_response = Some(resp);
            last_step.response_time = Some(elapsed);
        }
        Ok(())
    }

    fn run_pre_checks(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        if !self.config.checks.no_pre {
            self.run_checks(session, CheckMode::Pre, sender)
        } else {
            Ok(())
        }
    }

    fn run_post_checks(
        &self,
        session: &mut Session,
        sender: &Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        if session
            .steps()
            .last()
            .and_then(|s| s.model_response.as_ref())
            .is_some()
        {
            self.run_checks(session, CheckMode::Post, sender)?
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use crate::patch::{Change, Patch, WriteFile};
    use crate::session::ModelResponse;

    use fs_err as fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_new_session_with_no_context() {
        use crate::config::{Context, TextContext};
        let temp_dir = tempdir().unwrap();
        let mut config = Config::default().with_root(temp_dir.path());

        // Add just text context which doesn't require filesystem or parsing
        config.context = Context {
            ruskel: vec![],
            path: vec![],
            project_map: false,
            text: vec![TextContext {
                name: "test".to_string(),
                content: "test content".to_string(),
            }],
            cmd: vec![],
        };
        let tenx = Tenx::new(config);

        let session = tenx.new_session_from_cwd(&None, true).await.unwrap();
        assert!(session.contexts().is_empty());

        let session = tenx.new_session_from_cwd(&None, false).await.unwrap();
        assert!(!session.contexts().is_empty());
    }

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
                    response_text: Some("Test comment".to_string()),
                },
            ))
            .with_root(temp_dir.path());

        config.session_store_dir = temp_dir.path().join("sess");
        config.retry_limit = 1;
        config.project.globs.push("**".to_string());

        let tenx = Tenx::new(config.clone());
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "Initial content").unwrap();

        let mut session = Session::default();
        session
            .add_prompt(
                config.models.default.clone(),
                "Test prompt".to_string(),
                StepType::Code,
            )
            .unwrap();
        session
            .add_editable_path(&config, test_file_path.clone())
            .unwrap();

        tenx.process_prompt(&mut session, None).await.unwrap();

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
