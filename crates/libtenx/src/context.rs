use std::{collections::HashMap, path::PathBuf};

use crate::{Operation, Operations, Result, TenxError, Workspace};

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
    pub cache: HashMap<PathBuf, String>,
}

impl Context {
    pub(crate) fn new(
        edit_paths: Vec<PathBuf>,
        attach_paths: Vec<PathBuf>,
        user_prompt: String,
        workspace: Workspace,
    ) -> Result<Self> {
        let mut cache = HashMap::new();
        for path in &edit_paths {
            let contents = workspace.read_file(path)?;
            cache.insert(path.clone(), contents);
        }

        Ok(Context {
            edit_paths,
            attach_paths,
            user_prompt,
            workspace,
            cache,
        })
    }

    pub fn apply_all(&mut self, operations: &Operations) -> Result<()> {
        for operation in &operations.operations {
            self.apply(operation)?;
        }
        Ok(())
    }

    fn apply(&mut self, operation: &Operation) -> Result<()> {
        match operation {
            Operation::Replace(replace) => {
                // Get the current content from the cache
                let current_content = self.cache.get(&replace.path).ok_or_else(|| {
                    TenxError::Operation(format!(
                        "File '{}' not found in cache",
                        replace.path.display()
                    ))
                })?;

                // Apply the replacement
                let new_content = replace.apply(current_content)?;

                // Write to the workspace
                self.workspace.write_file(&replace.path, &new_content)?;
            }
            Operation::Write(write_file) => {
                // Write to the workspace
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
    use crate::{Operation, Replace, TenxError, Workspace, WriteFile};
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

        // Cache should remain unchanged
        assert_eq!(context.cache.get(&path).unwrap(), "Initial content");
        // Workspace should be updated
        assert_eq!(context.workspace.read_file(&path).unwrap(), "New content");
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

        // Cache should remain unchanged
        assert_eq!(context.cache.get(&path).unwrap(), "Initial content");
        // Workspace should be updated
        assert_eq!(
            context.workspace.read_file(&path).unwrap(),
            "Updated content"
        );
    }
}
