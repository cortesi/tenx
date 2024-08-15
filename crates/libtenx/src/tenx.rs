use std::{
    fs,
    path::{Path, PathBuf},
};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    model::ModelProvider, Operation, Operations, PromptInput, Result, Session, StateStore,
};

#[derive(Debug, Default)]
pub struct Config {
    pub anthropic_key: String,
    pub state_dir: Option<PathBuf>,
}

impl Config {
    /// Sets the Anthropic API key.
    pub fn with_anthropic_key(mut self, key: String) -> Self {
        self.anthropic_key = key;
        self
    }

    /// Sets the state directory.
    pub fn with_state_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.state_dir = Some(dir.as_ref().to_path_buf());
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

    /// Resets all files in the state snapshot to their original contents.
    pub fn reset(state: &Session) -> Result<()> {
        for (path, content) in &state.snapshot {
            fs::write(path, content)?;
        }
        Ok(())
    }

    /// Sends a prompt to the model and updates the state.
    pub async fn start(
        &self,
        state: &mut Session,
        prompt: PromptInput,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let state_store = StateStore::new(self.config.state_dir.as_ref())?;
        state_store.save(state)?;
        self.process_prompt(state, prompt, sender, &state_store)
            .await
    }

    /// Resumes a session by loading the state and sending a prompt to the model.
    pub async fn resume(
        &self,
        prompt: PromptInput,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let state_store = StateStore::new(self.config.state_dir.as_ref())?;
        let mut state = state_store.load(&std::env::current_dir()?)?;
        self.process_prompt(&mut state, prompt, sender, &state_store)
            .await
    }

    /// Common logic for processing a prompt and updating the state.
    async fn process_prompt(
        &self,
        state: &mut Session,
        mut prompt: PromptInput,
        sender: Option<mpsc::Sender<String>>,
        state_store: &StateStore,
    ) -> Result<()> {
        state.prompt_inputs.push(prompt.clone());
        for doc in &mut prompt.docs {
            doc.resolve()?;
        }
        let mut model = state.model.take().unwrap();
        let ops = model
            .prompt(&self.config, &state.dialect, state, sender)
            .await?;
        state.model = Some(model);
        match Self::apply_all(state, &ops) {
            Ok(_) => {
                state_store.save(state)?;
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

    fn apply_all(state: &mut Session, operations: &Operations) -> Result<()> {
        // Collect unique paths from operations
        let affected_paths: std::collections::HashSet<_> = operations
            .operations
            .iter()
            .map(|op| match op {
                Operation::Replace(replace) => &replace.path,
                Operation::Write(write) => &write.path,
            })
            .collect();

        // Process affected paths
        for path in affected_paths {
            if !state.snapshot.contains_key(path) {
                // If the file is not in the snapshot, read and store its contents
                let content = fs::read_to_string(path)?;
                state.snapshot.insert(path.to_path_buf(), content);
            } else {
                // If the file is in the snapshot, restore its contents to disk
                let content = state.snapshot.get(path).unwrap();
                fs::write(path, content)?;
            }
        }
        for operation in &operations.operations {
            Self::apply(operation)?;
        }
        Ok(())
    }

    fn apply(operation: &Operation) -> Result<()> {
        match operation {
            Operation::Replace(replace) => {
                let current_content = fs::read_to_string(&replace.path)?;
                let new_content = replace.apply(&current_content)?;
                fs::write(&replace.path, &new_content)?;
            }
            Operation::Write(write_file) => {
                fs::write(&write_file.path, &write_file.content)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialect::Dialect, model::Model, operations::Operation, operations::Replace};
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_start() -> Result<()> {
        let temp_dir = tempdir()?;
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Initial content")?;

        let config = Config::default().with_state_dir(temp_dir.path());
        let tenx = Tenx::new(config);
        let prompt = PromptInput {
            edit_paths: vec![file_path.clone()],
            user_prompt: "Test prompt".to_string(),
            ..Default::default()
        };

        let mut state = Session::new(
            temp_dir.path(),
            Dialect::Tags(crate::dialect::Tags::default()),
            Model::Dummy(crate::model::Dummy::new(Operations {
                operations: vec![Operation::Replace(Replace {
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
