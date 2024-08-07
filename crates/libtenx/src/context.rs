use std::path::PathBuf;

use crate::{Result, Workspace};

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
}

impl Context {
    pub(crate) fn new(
        edit_paths: Vec<PathBuf>,
        attach_paths: Vec<PathBuf>,
        user_prompt: String,
        workspace: Workspace,
    ) -> Self {
        Context {
            edit_paths,
            attach_paths,
            user_prompt,
            workspace,
        }
    }

    pub fn render(&self) -> Result<String> {
        let mut rendered = String::new();

        // Add editable files
        for path in &self.edit_paths {
            let contents = self.workspace.get_contents(path)?;
            rendered.push_str(&format!(
                "<editable path=\"{}\">\n{}</editable>\n\n",
                path.display(),
                contents
            ));
        }

        // Add context files
        for path in &self.attach_paths {
            let contents = self.workspace.get_contents(path)?;
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
