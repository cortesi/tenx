use std::{
    env,
    path::{Path, PathBuf},
};

use fs_err as fs;
use globset::Glob;
use pathdiff::diff_paths;
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
#[derive(Debug, Deserialize, Serialize)]
pub struct Session {
    /// The session root directory. This is always an absolute path. Context and editable files are
    /// always relative to the root.
    pub root: PathBuf,
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
    /// Creates a new Session with the specified root directory, dialect, and model.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: root.canonicalize().unwrap(),
            steps: vec![],
            context: vec![],
            editable: vec![],
        }
    }

    // Removed from_cwd method

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

    /// Calculates the relative path from the root to the given absolute path.
    pub fn relpath(&self, path: &Path) -> PathBuf {
        diff_paths(path, &self.root).unwrap_or_else(|| path.to_path_buf())
    }

    /// Converts a path relative to the root directory to an absolute path
    pub fn abspath(&self, path: &Path) -> Result<PathBuf> {
        self.root
            .join(path)
            .canonicalize()
            .map_err(|e| TenxError::Internal(format!("Could not canonicalize: {}", e)))
    }

    /// Returns the absolute paths of the editables for this session.
    pub fn abs_editables(&self) -> Result<Vec<PathBuf>> {
        self.editable
            .clone()
            .iter()
            .map(|p| self.abspath(p))
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

    /// Normalizes a path relative to the root directory.
    fn normalize_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        self.normalize_path_with_cwd(
            path,
            env::current_dir()
                .map_err(|e| TenxError::Internal(format!("Could not get cwd: {}", e)))?,
        )
    }

    /// Normalizes a path relative to the root directory with a given current working directory.
    fn normalize_path_with_cwd<P: AsRef<Path>>(
        &self,
        path: P,
        current_dir: PathBuf,
    ) -> Result<PathBuf> {
        let path = path.as_ref();
        if path.is_relative() {
            let absolute_path = current_dir
                .join(path)
                .canonicalize()
                .map_err(|e| TenxError::Internal(format!("Could not canonicalize: {}", e)))?;

            Ok(absolute_path
                .strip_prefix(&self.root)
                .unwrap_or(&absolute_path)
                .to_path_buf())
        } else {
            Ok(path.to_path_buf())
        }
    }

    /// Adds an editable file path to the session, normalizing relative paths.
    pub fn add_editable_path<P: AsRef<Path>>(&mut self, path: P) -> Result<usize> {
        let normalized_path = self.normalize_path(path)?;
        if !self.editable.contains(&normalized_path) {
            self.editable.push(normalized_path);
            Ok(1)
        } else {
            Ok(0)
        }
    }

    /// Helper function to match files based on a glob pattern
    pub fn match_files_with_glob(
        &self,
        config: &config::Config,
        pattern: &str,
    ) -> Result<Vec<PathBuf>> {
        let glob = Glob::new(pattern)
            .map_err(|e| TenxError::Internal(format!("Invalid glob pattern: {}", e)))?;
        let included_files = config.included_files(&self.root)?;

        let current_dir = env::current_dir()
            .map_err(|e| TenxError::Internal(format!("Failed to get current directory: {}", e)))?;

        let mut matched_files = Vec::new();

        for file in included_files {
            let relative_path = if file.is_absolute() {
                file.strip_prefix(&self.root).unwrap_or(&file)
            } else {
                &file
            };

            let match_path = if current_dir != self.root {
                // If we're in a subdirectory, we need to adjust the path for matching
                diff_paths(
                    relative_path,
                    current_dir
                        .strip_prefix(&self.root)
                        .unwrap_or(Path::new("")),
                )
                .unwrap_or_else(|| relative_path.to_path_buf())
            } else {
                relative_path.to_path_buf()
            };

            if glob.compile_matcher().is_match(&match_path) {
                let absolute_path = self.root.join(relative_path);
                if absolute_path.exists() {
                    matched_files.push(relative_path.to_path_buf());
                } else {
                    return Err(TenxError::Internal(format!(
                        "File does not exist: {:?}",
                        absolute_path
                    )));
                }
            }
        }

        Ok(matched_files)
    }

    /// Adds editable files to the session based on a glob pattern.
    pub fn add_editable_glob(&mut self, config: &config::Config, pattern: &str) -> Result<usize> {
        let matched_files = self.match_files_with_glob(config, pattern)?;
        let mut added = 0;
        for file in matched_files {
            added += self.add_editable_path(file)?;
        }
        Ok(added)
    }

    /// Adds context to the session, either as a single file or as a glob pattern.
    /// Adds an editable file or glob pattern to the session.
    pub fn add_editable(&mut self, config: &config::Config, path: &str) -> Result<usize> {
        if is_glob(path) {
            self.add_editable_glob(config, path)
        } else {
            self.add_editable_path(path)
        }
    }

    /// Apply a patch, entering the modified files into the patch cache. It is the caller's
    /// responsibility to save the patch back to the session if needed.
    pub fn apply_patch(&mut self, patch: &mut Patch) -> Result<()> {
        // First, enter all the modified files into the patch cache
        for path in patch.changed_files() {
            let abs_path = self.abspath(&path)?;
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
            let abs_path = self.abspath(&path)?;
            fs::write(&abs_path, content)?;
        }

        Ok(())
    }

    /// Rolls back the changes made by a patch, using the cached file contents.
    pub fn rollback(&self, patch: &Patch) -> Result<()> {
        for (path, content) in &patch.cache {
            fs::write(self.abspath(path)?, content)?;
        }
        Ok(())
    }

    /// Resets the session to a specific step, removing and rolling back all subsequent steps.
    pub fn reset(&mut self, offset: usize) -> Result<()> {
        if offset >= self.steps.len() {
            return Err(TenxError::Internal("Invalid rollback offset".into()));
        }

        for step in self.steps.iter().rev().take(self.steps.len() - offset - 1) {
            if let Some(patch) = &step.patch {
                self.rollback(patch)?;
            }
        }

        self.steps.truncate(offset + 1);
        Ok(())
    }

    /// Rolls back the changes in the last step, if any, and sets the Patch and error to None.
    pub fn rollback_last(&mut self) -> Result<()> {
        if let Some(patch) = self.steps.last().and_then(|step| step.patch.as_ref()) {
            self.rollback(patch)?;
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
    pub fn apply_last_patch(&mut self) -> Result<()> {
        let mut last_patch = self
            .steps
            .last()
            .and_then(|step| step.patch.clone())
            .ok_or_else(|| TenxError::Internal("No patch in the last step".into()))?;
        self.apply_patch(&mut last_patch)?;
        if let Some(last_step) = self.steps.last_mut() {
            last_step.patch = Some(last_patch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextProvider;
    use crate::patch::{Change, Patch, WriteFile};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_add_context_ignores_duplicates() {
        let temp_dir = tempdir().unwrap();
        let mut session = Session::new(temp_dir.path().to_path_buf());

        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "content").unwrap();

        let context1 = context::ContextSpec::new_glob("test.txt".to_string());
        let context2 = context::ContextSpec::new_glob("test.txt".to_string());

        session.add_context(context1.clone());
        session.add_context(context2);

        assert_eq!(session.context.len(), 1);
        assert_eq!(session.context[0].name(), "test.txt");

        // Create a mock config that doesn't rely on git
        let mut config = crate::config::Config::default();
        config.include = crate::config::Include::Glob(vec!["**/*".to_string()]);

        let context_items = session.context[0].contexts(&config, &session).unwrap();
        assert_eq!(context_items[0].body, "content");
    }

    #[test]
    fn test_normalize_path_with_cwd() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let sub_dir = root.join("subdir");
        fs::create_dir(&sub_dir).unwrap();

        let session = Session::new(root.clone());

        // Test 1: Current dir is the root directory
        {
            fs::File::create(root.join("file.txt")).unwrap();
            let result = session.normalize_path_with_cwd("file.txt", root.clone())?;
            assert_eq!(result, PathBuf::from("file.txt"));
        }

        // Test 2: Current dir is under the root directory
        {
            fs::File::create(sub_dir.join("subfile.txt")).unwrap();
            let result = session.normalize_path_with_cwd("subfile.txt", sub_dir.clone())?;
            assert_eq!(result, PathBuf::from("subdir/subfile.txt"));
        }

        // Test 3: Current dir is outside the root directory
        {
            let outside_dir = temp_dir.path().join("outside");
            fs::create_dir(&outside_dir).unwrap();
            fs::File::create(outside_dir.join("outsidefile.txt")).unwrap();
            let result = session.normalize_path_with_cwd("outsidefile.txt", outside_dir.clone())?;
            let expected = outside_dir
                .join("outsidefile.txt")
                .strip_prefix(&root)
                .unwrap_or(&outside_dir.join("outsidefile.txt"))
                .to_path_buf();
            assert_eq!(
                result.canonicalize().unwrap(),
                expected.canonicalize().unwrap()
            );
        }

        // Test 4: Absolute path
        {
            let abs_path = root.join("abs_file.txt");
            fs::File::create(&abs_path).unwrap();
            let result = session.normalize_path_with_cwd(&abs_path, root.clone())?;
            assert_eq!(result, abs_path);
        }

        Ok(())
    }

    #[test]
    fn test_reset() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let root_dir = temp_dir.path().to_path_buf();
        let file_path = root_dir.join("test.txt");

        let mut session = Session::new(root_dir.clone());

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
            session.apply_patch(&mut patch.clone())?;
        }

        assert_eq!(session.steps.len(), 3);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "Content 3");

        // Rollback to the first step
        session.reset(0)?;

        assert_eq!(session.steps.len(), 1);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "Content 1");

        Ok(())
    }

    #[test]
    fn test_match_files_with_glob() -> Result<()> {
        let temp_dir = tempdir()
            .map_err(|e| TenxError::Internal(format!("Failed to create temp dir: {}", e)))?;
        let root_dir = temp_dir
            .path()
            .canonicalize()
            .map_err(|e| TenxError::Internal(format!("Failed to canonicalize temp dir: {}", e)))?;
        let session = Session::new(root_dir.clone());

        // Create directory structure
        fs::create_dir_all(root_dir.join("src/subdir"))
            .map_err(|e| TenxError::Internal(format!("Failed to create src/subdir: {}", e)))?;
        fs::create_dir_all(root_dir.join("tests"))
            .map_err(|e| TenxError::Internal(format!("Failed to create tests dir: {}", e)))?;
        fs::write(root_dir.join("src/file1.rs"), "content1")
            .map_err(|e| TenxError::Internal(format!("Failed to write src/file1.rs: {}", e)))?;
        fs::write(root_dir.join("src/subdir/file2.rs"), "content2").map_err(|e| {
            TenxError::Internal(format!("Failed to write src/subdir/file2.rs: {}", e))
        })?;
        fs::write(root_dir.join("tests/test1.rs"), "test_content1")
            .map_err(|e| TenxError::Internal(format!("Failed to write tests/test1.rs: {}", e)))?;
        fs::write(root_dir.join("README.md"), "readme_content")
            .map_err(|e| TenxError::Internal(format!("Failed to write README.md: {}", e)))?;

        // Create a mock config
        let mut config = config::Config::default();
        config.include =
            config::Include::Glob(vec!["**/*.rs".to_string(), "README.md".to_string()]);

        // Test matching files from root directory
        let matched_files = session.match_files_with_glob(&config, "src/**/*.rs")?;
        assert_eq!(
            matched_files.len(),
            2,
            "Expected 2 matched files, got {}",
            matched_files.len()
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/file1.rs")),
            "src/file1.rs not matched"
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/subdir/file2.rs")),
            "src/subdir/file2.rs not matched"
        );

        // Store the original working directory
        let original_dir = env::current_dir()?;

        // Test matching files from subdirectory
        env::set_current_dir(root_dir.join("src"))?;
        let matched_files = session.match_files_with_glob(&config, "**/*.rs")?;
        assert_eq!(
            matched_files.len(),
            3,
            "Expected 3 matched files, got {}",
            matched_files.len()
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/file1.rs")),
            "src/file1.rs not matched"
        );
        assert!(
            matched_files.contains(&PathBuf::from("src/subdir/file2.rs")),
            "src/subdir/file2.rs not matched"
        );
        assert!(
            matched_files.contains(&PathBuf::from("tests/test1.rs")),
            "tests/test1.rs not matched"
        );

        // Test matching non-Rust files
        let matched_files = session.match_files_with_glob(&config, "../*.md")?;
        assert_eq!(
            matched_files.len(),
            1,
            "Expected 1 matched file, got {}",
            matched_files.len()
        );
        assert!(
            matched_files.contains(&PathBuf::from("README.md")),
            "README.md not matched"
        );

        // Reset the working directory
        env::set_current_dir(original_dir)?;

        Ok(())
    }
}
