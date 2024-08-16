use std::{
    fs,
    path::{Path, PathBuf},
};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{model::ModelProvider, Change, ChangeSet, PromptInput, Result, Session, SessionStore};

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
    pub fn reset(state: &Session) -> Result<()> {
        // FIXME
        Ok(())
    }

    /// Sends a prompt to the model and updates the state.
    pub async fn start(
        &self,
        state: &mut Session,
        prompt: PromptInput,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir.as_ref())?;
        session_store.save(state)?;
        self.process_prompt(state, prompt, sender, &session_store)
            .await
    }

    /// Resumes a session by loading the state and sending a prompt to the model.
    pub async fn resume(
        &self,
        prompt: PromptInput,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let session_store = SessionStore::open(self.config.session_store_dir.as_ref())?;
        let mut state = session_store.load(&std::env::current_dir()?)?;
        self.process_prompt(&mut state, prompt, sender, &session_store)
            .await
    }

    /// Common logic for processing a prompt and updating the state.
    async fn process_prompt(
        &self,
        state: &mut Session,
        prompt: PromptInput,
        sender: Option<mpsc::Sender<String>>,
        session_store: &SessionStore,
    ) -> Result<()> {
        state.prompt_inputs.push(prompt.clone());
        let mut model = state.model.take().unwrap();
        let ops = model
            .prompt(&self.config, &state.dialect, state, sender)
            .await?;
        state.model = Some(model);
        match Self::apply_all(state, &ops) {
            Ok(_) => {
                session_store.save(state)?;
                Ok(())
            }
            Err(e) => {
                warn!("{}", e);
                warn!("Resetting state...");
                Self::reset(state)?;
                Err(e)
            }
        }
    }

    fn apply_all(_state: &mut Session, change_set: &ChangeSet) -> Result<()> {
        for change in &change_set.changes {
            Self::apply(change)?;
        }
        Ok(())
    }

    fn apply(change: &Change) -> Result<()> {
        match change {
            Change::Replace(replace) => {
                let current_content = fs::read_to_string(&replace.path)?;
                let new_content = replace.apply(&current_content)?;
                fs::write(&replace.path, &new_content)?;
            }
            Change::Write(write_file) => {
                fs::write(&write_file.path, &write_file.content)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{changes::Change, changes::Replace, dialect::Dialect, model::Model};
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_start() -> Result<()> {
        let temp_dir = tempdir()?;
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Initial content")?;

        let config = Config::default().with_session_store_dir(temp_dir.path());
        let tenx = Tenx::new(config);
        let prompt = PromptInput {
            edit_paths: vec![file_path.clone()],
            user_prompt: "Test prompt".to_string(),
            ..Default::default()
        };

        let mut state = Session::new(
            Some(temp_dir.path().to_path_buf()),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::Dummy::new(ChangeSet {
                changes: vec![Change::Replace(Replace {
                    path: file_path.clone(),
                    old: "Initial content".to_string(),
                    new: "Updated content".to_string(),
                })],
            })),
        );
        state.prompt_inputs.push(prompt.clone());

        tenx.start(&mut state, prompt, None).await?;

        let updated_content = fs::read_to_string(&file_path)?;
        assert_eq!(updated_content, "Updated content");

        Ok(())
    }
}

