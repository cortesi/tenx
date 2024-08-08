use std::{collections::HashMap, path::PathBuf};

use crate::{Operation, Operations, Result, Workspace};

/// Defines the initial context of a conversation. This defines which files are editable, plus which
/// files and documentation will be provided as context.
#[derive(Debug)]
pub struct Context {
    /// Files to attach, but which the model can't edit
    pub attach_paths: Vec<PathBuf>,
    /// Editable paths
    pub edit_paths: Vec<PathBuf>,
    /// The user's initial prompt
    pub user_prompt: String,
    /// The workspace we're operating on
    pub workspace: Workspace,
    /// Cache of editable file contents
    pub snapshot: HashMap<PathBuf, String>,
}

impl Context {
    pub(crate) fn new(
        edit_paths: Vec<PathBuf>,
        attach_paths: Vec<PathBuf>,
        user_prompt: String,
        workspace: Workspace,
    ) -> Result<Self> {
        let mut snapshot = HashMap::new();
        for path in &edit_paths {
            let contents = workspace.read_file(path)?;
            snapshot.insert(path.clone(), contents);
        }

        Ok(Context {
            edit_paths,
            attach_paths,
            user_prompt,
            workspace,
            snapshot,
        })
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

        // Write snapshot contents to disk only for affected paths
        for path in affected_paths {
            if let Some(content) = self.snapshot.get(path) {
                self.workspace.write_file(path, content)?;
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
                let current_content = self.workspace.read_file(&replace.path)?;

                // Apply the replacement
                let new_content = replace.apply(&current_content)?;

                // Write the new content back to the file on disk
                self.workspace.write_file(&replace.path, &new_content)?;
            }
            Operation::Write(write_file) => {
                // Write operation directly writes to the file on disk
                self.workspace
                    .write_file(&write_file.path, &write_file.content)?;
            }
        }

        Ok(())
    }

    pub fn render(&self) -> Result<String> {
        let mut rendered = String::new();

        // Add editable files
        for path in &self.edit_paths {
            let contents = self.workspace.read_file(path)?;
            rendered.push_str(&format!(
                "<editable path=\"{}\">\n{}</editable>\n\n",
                path.display(),
                contents
            ));
        }

        // Add context files
        for path in &self.attach_paths {
            let contents = self.workspace.read_file(path)?;
            rendered.push_str(&format!(
                "<context path=\"{}\">\n{}</context>\n\n",
                path.display(),
                contents
            ));
        }

        // Add user prompt
        rendered.push_str(&self.user_prompt);

        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::{create_dummy_project, TempEnv};
    use crate::{Operation, Replace, WriteFile};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn setup_test_context() -> (TempDir, Context) {
        let temp_dir = TempDir::new().unwrap();
        create_dummy_project(temp_dir.path()).unwrap();
        let _temp_env = TempEnv::new(temp_dir.path()).unwrap();

        let workspace = Workspace::discover(&[temp_dir.path()]).unwrap();

        let edit_path = PathBuf::from("crate1/src/lib.rs");
        workspace.write_file(&edit_path, "Initial content").unwrap();

        let context = Context::new(
            vec![edit_path.clone()],
            vec![],
            "Test prompt".to_string(),
            workspace,
        )
        .unwrap();

        (temp_dir, context)
    }

    #[test]
    fn test_apply_write_operation() {
        let (_temp_dir, mut context) = setup_test_context();
        let path = PathBuf::from("crate1/src/lib.rs");

        let operation = Operation::Write(WriteFile {
            path: "crate1/src/lib.rs".into(),
            content: "New content".to_string(),
        });

        context.apply(&operation).unwrap();

        // Workspace should be updated
        assert_eq!(context.workspace.read_file(&path).unwrap(), "New content");
        // Snapshot should remain unchanged
        assert_eq!(context.snapshot.get(&path).unwrap(), "Initial content");
    }

    #[test]
    fn test_apply_replace_operation() {
        let (_temp_dir, mut context) = setup_test_context();
        let path = PathBuf::from("crate1/src/lib.rs");

        let operation = Operation::Replace(Replace {
            path: "crate1/src/lib.rs".into(),
            old: "Initial content".to_string(),
            new: "Updated content".to_string(),
        });

        context.apply(&operation).unwrap();

        // Workspace should be updated
        assert_eq!(
            context.workspace.read_file(&path).unwrap(),
            "Updated content"
        );
        // Snapshot should remain unchanged
        assert_eq!(context.snapshot.get(&path).unwrap(), "Initial content");
    }

    #[test]
    fn test_apply_multiple_operations() {
        let (_temp_dir, mut context) = setup_test_context();
        let path = PathBuf::from("crate1/src/lib.rs");

        let operations = Operations {
            operations: vec![
                Operation::Replace(Replace {
                    path: path.clone(),
                    old: "Initial content".to_string(),
                    new: "Updated content".to_string(),
                }),
                Operation::Replace(Replace {
                    path: path.clone(),
                    old: "Updated content".to_string(),
                    new: "Final content".to_string(),
                }),
            ],
        };

        context.apply_all(&operations).unwrap();

        // Check that the final content is correct
        assert_eq!(context.workspace.read_file(&path).unwrap(), "Final content");
        // Check that the snapshot remains unchanged
        assert_eq!(context.snapshot.get(&path).unwrap(), "Initial content");
    }

    #[test]
    fn test_apply_write_after_replace() {
        let (_temp_dir, mut context) = setup_test_context();
        let path = PathBuf::from("crate1/src/lib.rs");

        let operations = Operations {
            operations: vec![
                Operation::Replace(Replace {
                    path: path.clone(),
                    old: "Initial content".to_string(),
                    new: "Updated content".to_string(),
                }),
                Operation::Write(WriteFile {
                    path: path.clone(),
                    content: "Final content".to_string(),
                }),
            ],
        };

        context.apply_all(&operations).unwrap();

        // Check that the final content is correct
        assert_eq!(context.workspace.read_file(&path).unwrap(), "Final content");
        // Check that the snapshot remains unchanged
        assert_eq!(context.snapshot.get(&path).unwrap(), "Initial content");
    }
}
