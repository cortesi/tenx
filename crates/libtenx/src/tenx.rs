use std::path::PathBuf;
use tracing::warn;

use crate::{
    checks::{check_paths, check_session, CheckMode},
    config::Config,
    context::{Context, ContextProvider},
    events::{send_event, Event, EventBlock, EventSender},
    model::ModelProvider,
    session::{Action, Session},
    session_store::{path_to_filename, SessionStore},
    strategy,
    strategy::{ActionStrategy, Completion},
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
        sender: &Option<EventSender>,
        no_context: bool,
    ) -> Result<Session> {
        let _block = EventBlock::start(sender)?;
        let mut session = Session::new(&self.config)?;

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
        sender: &Option<EventSender>,
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
        sender: &Option<EventSender>,
    ) -> Result<()> {
        let _block = EventBlock::start(sender)?;
        self.refresh_contexts_inner(session, sender).await
    }

    /// Refreshes only contexts that need refreshing according to their needs_refresh() method.
    pub async fn refresh_needed_contexts(
        &self,
        session: &mut Session,
        sender: &Option<EventSender>,
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

    /// Add files to edit in the session and save it
    pub fn edit(&self, session: &mut Session, files: &[String]) -> Result<usize> {
        let (_, count) = session.state.view(&self.config.cwd()?, files.to_vec())?;
        self.save_session(session)?;
        Ok(count)
    }

    /// Adds a code action with the given prompt to the session.
    /// Files must be already added to the session with session.state.view() before calling this.
    pub fn code(&self, session: &mut Session) -> Result<()> {
        let action = Action::new(
            &self.config,
            strategy::Strategy::Code(strategy::Code::new()),
        )?;
        session.add_action(action)?;
        self.save_session(session)?;
        Ok(())
    }

    /// Adds a fix action to the session.
    /// Files must be already added to the session with session.state.view() before calling this.
    pub fn fix(&self, session: &mut Session, sender: &Option<EventSender>) -> Result<()> {
        let pre_result = self.run_pre_checks(session, sender);
        if let Err(e) = pre_result {
            let action = Action::new(&self.config, strategy::Strategy::Fix(strategy::Fix::new(e)))?;
            session.add_action(action)?;
            self.save_session(session)?;
            Ok(())
        } else {
            Err(TenxError::Internal("No errors found".to_string()))
        }
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

    /// Reverts the last step and prepares for retry.
    pub fn retry(&self, session: &mut Session) -> Result<()> {
        if let Some(step) = session.last_step() {
            session.state.revert(step.rollback_id)?;
        }
        self.save_session(session)?;
        Ok(())
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

    /// Run checks on specified paths.
    pub fn check(&self, paths: Vec<PathBuf>, sender: &Option<EventSender>) -> Result<()> {
        let _block = EventBlock::start(sender)?;
        check_paths(&self.config, &paths, CheckMode::Both, sender)
    }

    /// Take the next step for the current action.
    /// Returns the State of the current action after execution.
    pub async fn next_step(
        &self,
        session: &mut Session,
        prompt: Option<String>,
        sender: Option<EventSender>,
    ) -> Result<strategy::ActionState> {
        self.save_session(session)?;

        // If no action exists, we can't get a state, so return an error
        if session.last_action().is_none() {
            return Err(TenxError::Internal("No current action".to_string()));
        }

        // Get the initial state, and check if we're already complete
        {
            let action = session.last_action().unwrap();
            let state = action.strategy.state(&self.config, session);
            if matches!(state.completion, Completion::Complete) {
                return Ok(state);
            }
        }

        // Get the next step from the strategy
        let next_step = {
            let action = session.last_action().unwrap();
            action
                .strategy
                .next_step(&self.config, session, sender.clone(), prompt)?
        };

        // If there's no next step, just return the current state
        match next_step {
            Some(step) => {
                session.add_step(step)?;
                self.save_session(session)?;
            }
            None => {
                let action = session.last_action().unwrap();
                return Ok(action.strategy.state(&self.config, session));
            }
        }

        // Execute the step
        match self.execute_prompt_cycle(session, sender.clone()).await {
            Ok(()) => {
                self.save_session(session)?;
            }
            Err(e) => {
                if let Some(step) = session.last_step_mut() {
                    step.err = Some(e.clone());
                    self.save_session(session)?;
                }
            }
        }

        // Always return the current state from the action's strategy
        let action = session.last_action().unwrap();
        Ok(action.strategy.state(&self.config, session))
    }

    /// Iterate on steps until the action is complete.
    /// Returns the final state of the action.
    pub async fn iterate_steps(
        &self,
        session: &mut Session,
        prompt: Option<String>,
        sender: Option<EventSender>,
        timeout: Option<std::time::Duration>,
    ) -> Result<strategy::ActionState> {
        let _block = EventBlock::start(&sender)?;
        self.save_session(session)?;
        let mut step_count = 0;

        let start_time = std::time::Instant::now();
        loop {
            step_count += 1;

            // Use next_step to handle the strategy logic
            let state = self
                .next_step(
                    session,
                    if step_count == 1 {
                        prompt.clone()
                    } else {
                        None
                    },
                    sender.clone(),
                )
                .await?;

            // If the action is complete, we're done
            if state.should_stop_iteration() {
                return Ok(state);
            }

            // Check step limit
            if step_count >= self.config.step_limit {
                warn!("Step count limit reached");
                send_event(&sender, Event::IterationLimit)?;
                return Ok(state);
            }

            // Check timeout
            if let Some(timeout) = timeout {
                if start_time.elapsed() > timeout {
                    warn!("Timeout reached");
                    return Ok(state);
                }
            }
        }
    }

    async fn execute_prompt_cycle(
        &self,
        session: &mut Session,
        sender: Option<EventSender>,
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
    async fn prompt(&self, session: &mut Session, sender: Option<EventSender>) -> Result<()> {
        // FIXME: Get the model from the last step
        let mut model = self.config.active_model()?;
        let _block = EventBlock::prompt(&sender, &model.name())?;
        // FIXME: Make this param configurable
        let mut throttler = crate::throttle::Throttler::new(25);

        loop {
            let start_time = std::time::Instant::now();
            match model.send(&self.config, session, sender.clone()).await {
                Ok(resp) => {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    if let Some(last_step) = session.last_step_mut() {
                        last_step.model_response = Some(resp);
                        last_step.response_time = Some(elapsed);
                    }
                    throttler.reset();
                    return Ok(());
                }
                Err(TenxError::Throttle(t)) => {
                    throttler.throttle(&t, &sender).await?;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn run_pre_checks(&self, session: &mut Session, sender: &Option<EventSender>) -> Result<()> {
        if !self.config.checks.no_pre {
            let _check_block = EventBlock::pre_check(sender)?;
            check_session(&self.config, session, CheckMode::Pre, sender)
        } else {
            Ok(())
        }
    }

    fn run_post_checks(&self, session: &mut Session, sender: &Option<EventSender>) -> Result<()> {
        let _check_block = EventBlock::post_check(sender)?;
        if session
            .steps()
            .last()
            .and_then(|s| s.model_response.as_ref())
            .is_some()
        {
            check_session(&self.config, session, CheckMode::Post, sender)?
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
    use crate::strategy::{Completion, InputRequired};

    use fs_err as fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_new_session_with_no_context() -> Result<()> {
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

        let session = tenx.new_session_from_cwd(&None, false).await?;
        assert!(!session.contexts().is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_tenx_process_prompt() -> Result<()> {
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
                    raw_response: Some("Test comment".to_string()),
                },
            ))
            .with_root(temp_dir.path());

        config.session_store_dir = temp_dir.path().join("sess");
        config.step_limit = 1;
        config.project.include.push("**".to_string());

        let tenx = Tenx::new(config.clone());
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "Initial content").unwrap();

        let mut session = Session::new(&config).unwrap();

        // Add files to session first
        session
            .state
            .view(temp_dir.path().to_path_buf(), vec!["**".to_string()])
            .unwrap();

        // Create code action
        tenx.code(&mut session)?;

        // Run the steps
        tenx.iterate_steps(&mut session, Some("test".into()), None, None)
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
        Ok(())
    }

    #[tokio::test]
    async fn test_next_step_returns_state() -> Result<()> {
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
                    raw_response: Some("Test comment".to_string()),
                },
            ))
            .with_root(temp_dir.path());

        config.session_store_dir = temp_dir.path().join("sess");
        config.step_limit = 1;
        config.project.include.push("**".to_string());

        let tenx = Tenx::new(config.clone());
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "Initial content").unwrap();

        let mut session = Session::new(&config).unwrap();

        // Add files to session first
        session
            .state
            .view(temp_dir.path().to_path_buf(), vec!["**".to_string()])
            .unwrap();

        // Then add action
        session
            .add_action(Action::new(
                &config,
                strategy::Strategy::Code(strategy::Code::new()),
            )?)
            .unwrap();

        let state = tenx
            .next_step(&mut session, Some("test".into()), None)
            .await?;

        // Verify the returned state matches what we expect
        assert!(matches!(state.completion, Completion::Complete));
        assert!(matches!(state.input_required, InputRequired::No));

        // Also verify the step was executed properly
        assert_eq!(session.steps().len(), 1);
        assert!(session.steps()[0].model_response.is_some());
        let file_content = fs::read_to_string(&test_file_path).unwrap();
        assert_eq!(file_content, "Updated content");

        Ok(())
    }
}
