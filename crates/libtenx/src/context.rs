use std::{collections::HashMap, fs, path::PathBuf};

use crate::{Operation, Operations, Result};

#[derive(Debug, Default)]
pub struct Context {
    pub snapshot: HashMap<PathBuf, String>,
}

impl Context {
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
            if !self.snapshot.contains_key(path) {
                // If the file is not in the snapshot, read and store its contents
                let content = fs::read_to_string(path)?;
                self.snapshot.insert(path.to_path_buf(), content);
            } else {
                // If the file is in the snapshot, restore its contents to disk
                let content = self.snapshot.get(path).unwrap();
                fs::write(path, content)?;
            }
        }

        // Apply operations
        for operation in &operations.operations {
            self.apply(operation)?;
        }

        Ok(())
    }

    fn apply(&mut self, operation: &Operation) -> Result<()> {
        match operation {
            Operation::Replace(replace) => {
                // Read the current content from the file on disk
                let current_content = fs::read_to_string(&replace.path)?;

                // Apply the replacement
                let new_content = replace.apply(&current_content)?;

                // Write the new content back to the file on disk
                fs::write(&replace.path, &new_content)?;
            }
            Operation::Write(write_file) => {
                // Write operation directly writes to the file on disk
                fs::write(&write_file.path, &write_file.content)?;
            }
        }

        Ok(())
    }
}
