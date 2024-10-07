use std::path::{Path, PathBuf};

use fs_err as fs;
use serde::{Deserialize, Serialize};

use crate::{
    config, context, events::Event, model::ModelProvider, model::Usage, patch::Patch,
    prompt::Prompt, Result, TenxError,
};
use tokio::sync::mpsc;

/// A single step in the session - basically a prompt and a patch.
#[derive(Debug, Deserialize, Serialize)]
pub struct Step {
    pub prompt: Prompt,
    pub patch: Option<Patch>,
    pub err: Option<TenxError>,
    pub usage: Option<Usage>,
}

/// Determines if a given string is a glob pattern or a rooted path.
///
/// Returns false if the string starts with "./", indicating a rooted path.
/// Returns true otherwise, suggesting it might be a glob pattern.
fn is_glob(s: &str) -> bool {
    !s.starts_with("./")
}

/// A serializable session, which persists between invocations.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Session {
    steps: Vec<Step>,
    pub(crate) context: Vec<context::ContextSpec>,
    editable: Vec<PathBuf>,
}

impl Session {
    /// Updates the prompt at a specific step.
    pub fn update_prompt_at(&mut self, offset: usize, prompt: Prompt) -> Result<()> {
        if offset >= self.steps.len() {
            return Err(TenxError::Internal("Invalid step offset".into()));
        }
        self.steps[offset].prompt = prompt;
        Ok(())
    }

    /// Clears all steps in the session, but keeps the current editable and context intact.
    pub fn clear(&mut self) {
        self.steps.clear();
    }
}

impl Session {
    pub fn steps(&self) -> &Vec<Step> {
        &self.steps
    }

    pub fn context(&self) -> &Vec<context::ContextSpec> {
        &self.context
    }

    /// Returns the relative paths of the editables for this session.
    pub fn editable(&self) -> &Vec<PathBuf> {
        &self.editable
    }

    /// Returns the absolute paths of the editables for this session.
    pub fn abs_editables(&self, config: &config::Config) -> Result<Vec<PathBuf>> {
        self.editable
            .clone()
            .iter()
            .map(|p| config.abspath(p))
            .collect()
    }

    /// Does this session have a pending prompt?
    pub fn pending_prompt(&self) -> bool {
        if let Some(step) = self.steps.last() {
            step.patch.is_none()
        } else {
            false
        }
    }

    /// Return the error if the last step has one, else None.
    pub fn last_step_error(&self) -> Option<&TenxError> {
        self.steps.last().and_then(|step| step.err.as_ref())
    }

    /// Adds a patch to the final step
    pub fn set_last_patch(&mut self, patch: &Patch) {
        if let Some(step) = self.steps.last_mut() {
            step.patch = Some(patch.clone());
        }
    }

    /// Adds an error to the final step
    pub fn set_last_error(&mut self, err: &TenxError) {
        if let Some(step) = self.steps.last_mut() {
            step.err = Some(err.clone());
        }
    }

    /// Adds a new step to the session, and sets the step prompt.
    ///
    /// Returns an error if the last step doesn't have either a patch or an error.
    pub fn add_prompt(&mut self, prompt: Prompt) -> Result<()> {
        if let Some(last_step) = self.steps.last() {
            if last_step.patch.is_none() && last_step.err.is_none() {
                return Err(TenxError::Internal(
                    "Cannot add a new prompt while the previous step is incomplete".into(),
                ));
            }
        }
        self.steps.push(Step {
            prompt,
            patch: None,
            err: None,
            usage: None,
        });
        Ok(())
    }

    /// Sets the prompt for the last step in the session.
    /// If there are no steps, it creates a new one.
    pub fn set_last_prompt(&mut self, prompt: Prompt) -> Result<()> {
        if self.steps.is_empty() {
            self.steps.push(Step {
                prompt,
                patch: None,
                err: None,
                usage: None,
            });
            Ok(())
        } else if let Some(last_step) = self.steps.last_mut() {
            last_step.prompt = prompt;
            Ok(())
        } else {
            Err(TenxError::Internal("Failed to set prompt".into()))
        }
    }

    /// Adds a new context to the session, ignoring duplicates.
    ///
    /// If a context with the same name and type already exists, it will not be added again.
    pub fn add_context(&mut self, new_context: context::ContextSpec) {
        if !self.context.contains(&new_context) {
            self.context.push(new_context);
        }
    }

    /// Adds an editable file path to the session, normalizing relative paths.
    pub fn add_editable_path<P: AsRef<Path>>(
        &mut self,
        config: &config::Config,
        path: P,
    ) -> Result<usize> {
        let normalized_path = config.normalize_path(path)?;
        if !self.editable.contains(&normalized_path) {
            self.editable.push(normalized_path);
            Ok(1)
        } else {
            Ok(0)
        }
    }

    /// Adds editable files to the session based on a glob pattern.
    pub fn add_editable_glob(&mut self, config: &config::Config, pattern: &str) -> Result<usize> {
        let matched_files = config.match_files_with_glob(pattern)?;
        let mut added = 0;
        for file in matched_files {
            added += self.add_editable_path(config, file)?;
        }
        Ok(added)
    }

    /// Adds context to the session, either as a single file or as a glob pattern.
    /// Adds an editable file or glob pattern to the session.
    pub fn add_editable(&mut self, config: &config::Config, path: &str) -> Result<usize> {
        if is_glob(path) {
            self.add_editable_glob(config, path)
        } else {
            self.add_editable_path(config, path)
        }
    }

    /// Apply a patch, entering the modified files into the patch cache. It is the caller's
    /// responsibility to save the patch back to the session if needed.
    pub fn apply_patch(&mut self, config: &config::Config, patch: &mut Patch) -> Result<()> {
        // First, enter all the modified files into the patch cache
        for path in patch.changed_files() {
            let abs_path = config.abspath(&path)?;
            if let std::collections::hash_map::Entry::Vacant(e) = patch.cache.entry(path) {
                let content = fs::read_to_string(&abs_path)?;
                e.insert(content);
            }
        }

        // Next, make a clone copy of the cache
        let mut modified_cache = patch.cache.clone();

        // Apply all modifications to the cloned cache
        patch.apply(&mut modified_cache)?;

        // Finally, write all files to disk
        for (path, content) in modified_cache {
            let abs_path = config.abspath(&path)?;
            fs::write(&abs_path, content)?;
        }

        Ok(())
    }

    /// Rolls back the changes made by a patch, using the cached file contents.
    pub fn rollback(&self, config: &config::Config, patch: &Patch) -> Result<()> {
        for (path, content) in &patch.cache {
            fs::write(config.abspath(path)?, content)?;
        }
        Ok(())
    }

    /// Resets the session to a specific step, removing and rolling back all subsequent steps.
    pub fn reset(&mut self, config: &config::Config, offset: usize) -> Result<()> {
        if offset >= self.steps.len() {
            return Err(TenxError::Internal("Invalid rollback offset".into()));
        }

        for step in self.steps.iter().rev().take(self.steps.len() - offset - 1) {
            if let Some(patch) = &step.patch {
                self.rollback(config, patch)?;
            }
        }

        self.steps.truncate(offset + 1);
        Ok(())
    }

    /// Rolls back the changes in the last step, if any, and sets the Patch and error to None.
    pub fn rollback_last(&mut self, config: &config::Config) -> Result<()> {
        if let Some(patch) = self.steps.last().and_then(|step| step.patch.as_ref()) {
            self.rollback(config, patch)?;
        }
        if let Some(last_step) = self.steps.last_mut() {
            last_step.patch = None;
            last_step.err = None;
        }
        Ok(())
    }

    /// Prompts the current model with the session's state and sets the resulting patch and usage.
    pub async fn prompt(
        &mut self,
        config: &config::Config,
        sender: Option<mpsc::Sender<Event>>,
    ) -> Result<()> {
        let mut model = config.model()?;
        let (patch, usage) = model.send(config, self, sender).await?;
        if let Some(last_step) = self.steps.last_mut() {
            last_step.patch = Some(patch);
            last_step.usage = Some(usage);
        }
        Ok(())
    }

    /// Applies the final patch in the session.
    pub fn apply_last_patch(&mut self, config: &config::Config) -> Result<()> {
        let mut last_patch = self
            .steps
            .last()
            .and_then(|step| step.patch.clone())
            .ok_or_else(|| TenxError::Internal("No patch in the last step".into()))?;
        self.apply_patch(config, &mut last_patch)?;
        if let Some(last_step) = self.steps.last_mut() {
            last_step.patch = Some(last_patch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::ProjectRoot,
        context::ContextProvider,
        patch::{Change, Patch, WriteFile},
    };
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_add_context_ignores_duplicates() {
        let temp_dir = tempdir().unwrap();
        let mut session = Session::default();

        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "content").unwrap();

        let context1 = context::ContextSpec::Glob(context::Glob::new("test.txt".to_string()));
        let context2 = context::ContextSpec::Glob(context::Glob::new("test.txt".to_string()));

        session.add_context(context1.clone());
        session.add_context(context2);

        assert_eq!(session.context.len(), 1);
        assert!(matches!(session.context[0], context::ContextSpec::Glob(_)));

        // Create a mock config that doesn't rely on git
        let mut config = crate::config::Config::default();
        config.include = crate::config::Include::Glob(vec!["**/*".to_string()]);
        config.project_root = crate::config::ProjectRoot::Path(temp_dir.path().to_path_buf());

        if let context::ContextSpec::Glob(glob_context) = &session.context[0] {
            let context_items = glob_context.contexts(&config, &session).unwrap();
            assert_eq!(context_items[0].body, "content");
        } else {
            panic!("Expected Glob context");
        }
    }

    #[test]
    fn test_reset() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let root_dir = temp_dir.path().to_path_buf();
        let file_path = root_dir.join("test.txt");
        let mut config = crate::config::Config::default();

        config.project_root = ProjectRoot::Path(root_dir.clone());

        let mut session = Session::default();

        // Create initial file
        fs::write(&file_path, "Initial content").unwrap();

        // Add three steps
        for i in 1..=3 {
            let content = format!("Content {}", i);
            let patch = Patch {
                changes: vec![Change::Write(WriteFile {
                    path: PathBuf::from("test.txt"),
                    content: content.clone(),
                })],
                comment: Some(format!("Step {}", i)),
                cache: [(
                    PathBuf::from("test.txt"),
                    fs::read_to_string(&file_path).unwrap(),
                )]
                .into_iter()
                .collect(),
            };
            session.add_prompt(Prompt::User(format!("Prompt {}", i)))?;
            session.set_last_patch(&patch);
            session.apply_patch(&config, &mut patch.clone())?;
        }

        assert_eq!(session.steps.len(), 3);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "Content 3");

        // Rollback to the first step
        session.reset(&config, 0)?;

        assert_eq!(session.steps.len(), 1);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "Content 1");

        Ok(())
    }
}
