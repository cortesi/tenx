use std::{
    fs,
    path::{Path, PathBuf},
};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{model::Model, Operation, Operations, Prompt, Result, State, StateStore};

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
    pub fn reset(state: &State) -> Result<()> {
        for (path, content) in &state.snapshot {
            fs::write(path, content)?;
        }
        Ok(())
    }

    /// Sends a prompt to the model and updates the state.
    pub async fn start(
        &self,
        state: &mut State,
        mut prompt: Prompt,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let state_store = StateStore::new(self.config.state_dir.as_ref())?;
        state_store.save(state)?;

        for doc in &mut prompt.docs {
            doc.resolve()?;
        }
        let ops = state
            .model
            .prompt(&self.config, &state.dialect, &prompt, sender)
            .await?;
        if let Err(e) = Self::apply_all(state, &ops) {
            warn!("{}", e);
            warn!("Resetting state...");
            Self::reset(state)?;
        } else {
            state_store.save(state)?;
        }
        Ok(())
    }

    fn apply_all(state: &mut State, operations: &Operations) -> Result<()> {
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
