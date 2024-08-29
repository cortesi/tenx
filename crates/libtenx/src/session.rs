use std::{
    env, fs,
    path::{Path, PathBuf},
};

use libruskel::Ruskel;
use pathdiff::diff_paths;
use serde::{Deserialize, Serialize};

use crate::{
    dialect::Dialect,
    events::Event,
    model::ModelProvider,
    patch::{Change, Patch},
    prompt::Prompt,
    Config, Result, TenxError,
};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextType {
    Ruskel,
    File,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ContextData {
    /// Unresolved content that should be read from a file each time the session is rendered.
    Path(PathBuf),
    /// Resolved content that can be passed to the model.
    String(String),
}

/// Reference material included in the prompt.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Context {
    /// The type of documentation.
    pub ty: ContextType,
    /// The name of the documentation.
    pub name: String,
    /// The contents of the help document.
    pub data: ContextData,
}

impl Context {
    /// Converts a Docs to a string representation.
    pub fn body(&self, session: &Session) -> Result<String> {
        match &self.data {
            ContextData::String(content) => Ok(content.clone()),
            ContextData::Path(path) => Ok(std::fs::read_to_string(session.abspath(path)?)
                .map_err(|e| TenxError::fio(e, path.clone()))?),
        }
    }
}

/// Finds the root directory based on the current working directory or git repo root.
pub fn find_root(current_dir: &Path) -> PathBuf {
    let mut dir = current_dir.to_path_buf();
    loop {
        if dir.join(".git").is_dir() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    current_dir.to_path_buf()
}

use crate::model::Usage;

/// A single step in the session - basically a prompt and a patch.
#[derive(Debug, Deserialize, Serialize)]
pub struct Step {
    pub prompt: Prompt,
    pub patch: Option<Patch>,
    pub err: Option<TenxError>,
    pub usage: Option<Usage>,
}

/// A serializable session, which persists between invocations.
#[derive(Debug, Deserialize, Serialize)]
pub struct Session {
    /// The session root directory. This is always an absolute path. Context and editable files are
    /// always relative to the root.
    pub root: PathBuf,
    /// The dialect used in the session
    pub dialect: Dialect,
    steps: Vec<Step>,
    context: Vec<Context>,
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
}

impl Session {
    /// Creates a new Session with the specified root directory, dialect, and model.
    pub fn new(root: PathBuf, dialect: Dialect) -> Self {
        Self {
            root: root.canonicalize().unwrap(),
            dialect,
            steps: vec![],
            context: vec![],
            editable: vec![],
        }
    }

    /// Creates a new Session, discovering the root from the current working directory. At the
    /// moment, this means the enclosing git repository, if there is one, otherwise the current
    /// directory.
    pub fn from_cwd(dialect: Dialect) -> Result<Self> {
        let cwd = env::current_dir().map_err(|e| TenxError::fio(e, "."))?;
        let root = find_root(&cwd);
        Ok(Self::new(root, dialect))
    }

    pub fn steps(&self) -> &Vec<Step> {
        &self.steps
    }

    pub fn context(&self) -> &Vec<Context> {
        &self.context
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
            .map_err(|e| TenxError::fio(e, path))
    }

    /// Returns the absolute paths of the editables for this session.
    pub fn editables(&self) -> Result<Vec<PathBuf>> {
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

    /// Adds a patch to the final step
    pub fn set_last_patch(&mut self, patch: &Patch) {
        if let Some(step) = self.steps.last_mut() {
            step.patch = Some(patch.clone());
        }
    }

    /// Adds a patch to the final step
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
    pub fn set_last_prompt(&mut self, prompt: Prompt) -> Result<()> {
        if let Some(last_step) = self.steps.last_mut() {
            last_step.prompt = prompt;
            Ok(())
        } else {
            Err(TenxError::Internal("No steps in the session".into()))
        }
    }

    /// Adds a new context to the session, ignoring duplicates.
    ///
    /// If a context with the same name and type already exists, it will not be added again.
    pub fn add_context(&mut self, context: Context) {
        if !self
            .context
            .iter()
            .any(|c| c.name == context.name && c.ty == context.ty)
        {
            self.context.push(context);
        }
    }

    /// Normalizes a path relative to the root directory.
    fn normalize_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        self.normalize_path_with_cwd(
            path,
            env::current_dir().map_err(|e| TenxError::fio(e, "."))?,
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
                .map_err(|e| TenxError::fio(e, path))?;

            Ok(absolute_path
                .strip_prefix(&self.root)
                .unwrap_or(&absolute_path)
                .to_path_buf())
        } else {
            Ok(path.to_path_buf())
        }
    }

    /// Adds a file path context to the session, normalizing relative paths.
    pub fn add_ctx_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let normalized_path = self.normalize_path(path)?;
        self.add_context(Context {
            ty: ContextType::File,
            name: normalized_path.to_string_lossy().into_owned(),
            data: ContextData::Path(normalized_path),
        });
        Ok(())
    }

    /// Adds an editable file path to the session, normalizing relative paths.
    pub fn add_editable<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let normalized_path = self.normalize_path(path)?;
        if !self.editable.contains(&normalized_path) {
            self.editable.push(normalized_path);
        }
        Ok(())
    }

    /// Adds a Ruskel context to the session and resolves it.
    pub fn add_ctx_ruskel(&mut self, name: String) -> Result<()> {
        let ruskel = Ruskel::new(&name);
        let resolved = ruskel
            .render(false, false)
            .map_err(|e| TenxError::Resolve(e.to_string()))?;

        self.add_context(Context {
            ty: ContextType::Ruskel,
            name,
            data: ContextData::String(resolved),
        });
        Ok(())
    }

    /// Apply a patch, entering the modified files into the patch cache. It is the caller's
    /// responsibility to save the patch back the the sesison if needed.
    pub fn apply_patch(&mut self, patch: &mut Patch) -> Result<()> {
        // First, enter all the modified files into the patch cache
        for path in patch.changed_files() {
            let abs_path = self.abspath(&path)?;
            if let std::collections::hash_map::Entry::Vacant(e) = patch.cache.entry(path) {
                let content = fs::read_to_string(&abs_path)
                    .map_err(|e| TenxError::fio(e, abs_path.clone()))?;
                e.insert(content);
            }
        }

        // Next, make a clone copy of the cache
        let mut modified_cache = patch.cache.clone();

        // Now all modifications are applied to the cloned cache one after the other
        for change in &patch.changes {
            match change {
                Change::Replace(replace) => {
                    let current_content = modified_cache.get(&replace.path).ok_or_else(|| {
                        TenxError::Internal("File not found in cache".to_string())
                    })?;
                    let new_content = replace.apply(current_content)?;
                    modified_cache.insert(replace.path.clone(), new_content);
                }
                Change::Write(write_file) => {
                    modified_cache.insert(write_file.path.clone(), write_file.content.clone());
                }
                Change::Block(_) => {}
            }
        }

        // Finally, write all files to disk
        for (path, content) in modified_cache {
            let abs_path = self.abspath(&path)?;
            fs::write(&abs_path, content).map_err(|e| TenxError::fio(e, abs_path.clone()))?;
        }

        Ok(())
    }

    /// Rolls back the changes made by a patch, using the cached file contents.
    pub fn rollback(&self, patch: &Patch) -> Result<()> {
        for (path, content) in &patch.cache {
            fs::write(self.abspath(path)?, content).map_err(|e| TenxError::fio(e, path.clone()))?;
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
        config: &Config,
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
    use crate::patch::{Change, Patch, WriteFile};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_add_context_ignores_duplicates() {
        let temp_dir = tempdir().unwrap();
        let mut session = Session::new(
            temp_dir.path().to_path_buf(),
            Dialect::Tags(crate::dialect::Tags {}),
        );

        let context1 = Context {
            ty: ContextType::File,
            name: "test.txt".to_string(),
            data: ContextData::String("content".to_string()),
        };
        let context2 = Context {
            ty: ContextType::File,
            name: "test.txt".to_string(),
            data: ContextData::String("different content".to_string()),
        };

        session.add_context(context1.clone());
        session.add_context(context2);

        assert_eq!(session.context.len(), 1);
        assert_eq!(session.context[0].name, "test.txt");
        assert_eq!(session.context[0].body(&session).unwrap(), "content");
    }

    #[test]
    fn test_normalize_path_with_cwd() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let sub_dir = root.join("subdir");
        fs::create_dir(&sub_dir).unwrap();

        let session = Session::new(root.clone(), Dialect::Tags(crate::dialect::Tags {}));

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

        let mut session = Session::new(root_dir.clone(), Dialect::Tags(crate::dialect::Tags {}));

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
}
