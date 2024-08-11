use std::{fs, path::Path};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    dialect::Dialects,
    model::{Model, Models},
    Operation, Operations, Prompt, Result, State,
};

#[derive(Debug, Default)]
pub struct Config {
    pub anthropic_key: String,
}

/// Tenx is an AI-driven coding assistant.
pub struct Tenx {
    pub state: State,
    pub config: Config,
}

impl Tenx {
    /// Creates a new Context with the specified working directory and dialect.
    pub fn new<P: AsRef<Path>>(working_directory: P, dialect: Dialects, model: Models) -> Self {
        Self {
            state: State::new(working_directory, dialect, model),
            config: Config::default(),
        }
    }

    /// Sets the Anthropic API key.
    pub fn with_anthropic_key(mut self, key: String) -> Self {
        self.config.anthropic_key = key;
        self
    }

    /// Resets all files in the state snapshot to their original contents.
    pub fn reset(&self) -> Result<()> {
        for (path, content) in &self.state.snapshot {
            fs::write(path, content)?;
        }
        Ok(())
    }

    /// Sends a prompt to the model.
    pub async fn prompt(
        &mut self,
        prompt: &Prompt,
        sender: Option<mpsc::Sender<String>>,
    ) -> Result<()> {
        let ops = self
            .state
            .model
            .prompt(&self.config, &self.state.dialect, prompt, sender)
            .await?;
        if let Err(e) = self.apply_all(&ops) {
            warn!("{}", e);
            warn!("Resetting state...");
            self.reset()?;
        }
        Ok(())
    }

    fn apply_all(&mut self, operations: &Operations) -> Result<()> {
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
            if !self.state.snapshot.contains_key(path) {
                // If the file is not in the snapshot, read and store its contents
                let content = fs::read_to_string(path)?;
                self.state.snapshot.insert(path.to_path_buf(), content);
            } else {
                // If the file is in the snapshot, restore its contents to disk
                let content = self.state.snapshot.get(path).unwrap();
                fs::write(path, content)?;
            }
        }
        for operation in &operations.operations {
            self.apply(operation)?;
        }
        Ok(())
    }

    fn apply(&mut self, operation: &Operation) -> Result<()> {
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
