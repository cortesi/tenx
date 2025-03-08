use std::path::PathBuf;
use tracing::warn;

use crate::{
    checks::{check_all, check_paths},
    config::Config,
    context::{Context, ContextProvider},
    error::{Result, TenxError},
    events::{send_event, Event, EventBlock, EventSender},
    model::ModelProvider,
    session::{Action, Session},
    session_store::{path_to_filename, SessionStore},
    strategy,
    strategy::{ActionStrategy, Completion},
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
        let (_, count) = session
            .last_action_mut()?
            .state
            .view(&self.config.cwd()?, files.to_vec())?;
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

    /// Reverts to a specific step and prepares for retry.
    ///
    /// * `action_idx` - Optional 0-based index of the action
    /// * `step_idx` - Optional 0-based index of the step within the action
    ///
    /// If both indices are None, uses the last step of the last action.
    pub fn retry(
        &self,
        session: &mut Session,
        action_idx: Option<usize>,
        step_idx: Option<usize>,
    ) -> Result<()> {
        if session.actions.is_empty() {
            return Err(TenxError::Internal("No actions in session".to_string()));
        }

        // Determine which action and step to use
        let action_index = action_idx.unwrap_or_else(|| session.actions.len() - 1);

        // If step_idx is None, use the last step of the specified action
        let step_index = if let Some(idx) = step_idx {
            idx
        } else {
            let steps = session.actions[action_index].steps();
            if steps.is_empty() {
                return Err(TenxError::Internal("No steps in action".to_string()));
            }
            steps.len() - 1
        };

        // Reset to this step and prepare it for retry
        session.retry(action_index, step_index)?;
        self.save_session(session)?;
        Ok(())
    }

    /// Resets the session to a specific action and step.
    ///
    /// * `action_idx` - The 0-based index of the action
    /// * `step_idx` - The 0-based index of the step within the action
    pub fn reset(
        &self,
        session: &mut Session,
        action_idx: usize,
        step_idx: Option<usize>,
    ) -> Result<()> {
        session.reset(action_idx, step_idx)?;
        self.save_session(session)
    }

    /// Resets all steps in the session.
    pub fn reset_all(&self, session: &mut Session) -> Result<()> {
        session.reset_all()?;
        self.save_session(session)
    }

    /// Run checks on specified paths.
    pub fn check(&self, paths: Vec<PathBuf>, sender: &Option<EventSender>) -> Result<()> {
        let _block = EventBlock::start(sender)?;
        if paths.is_empty() {
            check_all(&self.config, sender)
        } else {
            check_paths(&self.config, &paths, sender)
        }
    }

    /// Take the next step for the current action.
    /// Returns the State of the current action after execution.
    async fn next_step(
        &self,
        session: &mut Session,
        prompt: Option<String>,
        sender: Option<EventSender>,
    ) -> Result<strategy::ActionState> {
        self.save_session(session)?;

        let action = session
            .actions
            .last()
            .ok_or_else(|| TenxError::Internal("No actions in session".to_string()))?;
        let action_offset = session.actions.len() - 1;
        let state = action.strategy.state(&self.config, session, action_offset);
        if matches!(state.completion, Completion::Complete) {
            return Ok(state);
        }

        // First get the strategy and action offset without holding an immutable reference
        let action_offset = session.actions.len() - 1;
        let strategy = action.strategy.clone();

        // Now call next_step with the mutable session reference
        let next_step =
            strategy.next_step(&self.config, session, action_offset, sender.clone(), prompt)?;

        // Save state after the strategy generates the next step
        self.save_session(session)?;

        // If the action is done or requires user input, return early
        if next_step.should_stop_iteration() {
            return Ok(next_step);
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

        Ok(session.actions[action_offset]
            .strategy
            .state(&self.config, session, action_offset))
    }

    /// Iterate on steps until the action is complete.
    /// The optional prompt is passed to the first step.
    /// Returns the final state of the action.
    pub async fn continue_steps(
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
            let action_state = self
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
            if action_state.should_stop_iteration() {
                return Ok(action_state);
            }

            // Check step limit
            if step_count >= self.config.step_limit {
                warn!("Step count limit reached");
                send_event(&sender, Event::IterationLimit)?;
                return Ok(action_state);
            }

            // Check timeout
            if let Some(timeout) = timeout {
                if start_time.elapsed() > timeout {
                    warn!("Timeout reached");
                    return Ok(action_state);
                }
            }
        }
    }

    async fn execute_prompt_cycle(
        &self,
        session: &mut Session,
        sender: Option<EventSender>,
    ) -> Result<()> {
        self.prompt_model(session, sender.clone()).await?;
        send_event(&sender, Event::ApplyPatch)?;
        session.apply_last_step(&self.config)?;
        if !session.should_continue() {
            // We're done, now we check if checks return an error we need to process
            self.run_post_checks(session, &sender)?;
        }
        Ok(())
    }

    /// Prompts the current model with the session's state and sets the resulting patch and usage.
    async fn prompt_model(&self, session: &mut Session, sender: Option<EventSender>) -> Result<()> {
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
            let action = session.last_action()?;
            let strategy = action.strategy.clone();
            strategy.check(
                &self.config,
                session,
                session.actions.len() - 1,
                sender.clone(),
            )
        } else {
            Ok(())
        }
    }

    fn run_post_checks(&self, session: &mut Session, sender: &Option<EventSender>) -> Result<()> {
        let _check_block = EventBlock::post_check(sender)?;
        let action = session.last_action()?;
        let strategy = action.strategy.clone();
        strategy.check(
            &self.config,
            session,
            session.actions.len() - 1,
            sender.clone(),
        )
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
        assert!(session.contexts.is_empty());

        let session = tenx.new_session_from_cwd(&None, false).await?;
        assert!(!session.contexts.is_empty());
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

        // Create code action
        tenx.code(&mut session)?;

        // Then add files to session first
        session
            .last_action_mut()
            .unwrap()
            .state
            .view(temp_dir.path().to_path_buf(), vec!["**".to_string()])
            .unwrap();

        // Run the steps
        tenx.continue_steps(&mut session, Some("test".into()), None, None)
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

        // Add action
        session
            .add_action(Action::new(
                &config,
                strategy::Strategy::Code(strategy::Code::new()),
            )?)
            .unwrap();

        // Then add files to session
        session
            .last_action_mut()
            .unwrap()
            .state
            .view(temp_dir.path().to_path_buf(), vec!["**".to_string()])
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
