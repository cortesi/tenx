use std::{fs, path::Path};

use crate::{Operation, Operations, Result, State};

/// Tenx is an AI-driven coding assistant.
pub struct Tenx {
    state: State,
    anthropic_key: String,
}

impl Tenx {
    /// Creates a new Context with the specified working directory.
    pub fn new<P: AsRef<Path>>(working_directory: P) -> Self {
        Self {
            state: State::new(working_directory),
            anthropic_key: String::new(),
        }
    }

    /// Sets the Anthropic API key.
    pub fn with_anthropic_key(mut self, key: String) -> Self {
        self.anthropic_key = key;
        self
    }

    /// Resets all files in the state snapshot to their original contents.
    pub fn reset(&self) -> Result<()> {
        for (path, content) in &self.state.snapshot {
            fs::write(path, content)?;
        }
        Ok(())
    }

    pub fn apply_all(&mut self, operations: &Operations) -> Result<()> {
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

    pub fn apply(&mut self, operation: &Operation) -> Result<()> {
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
