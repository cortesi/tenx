use std::{
    env, fs,
    path::{Path, PathBuf},
};

use libruskel::Ruskel;
use pathdiff::diff_paths;
use serde::{Deserialize, Serialize};

use crate::{
    dialect::Dialect,
    model::Model,
    patch::{Change, Patch},
    prompt::PromptInput,
    Result, TenxError,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextType {
    Ruskel,
    File,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ContextData {
    /// Unresolved content that should be read from a file
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

/// Finds the working directory based on the given path or git repo root.
pub fn find_root<P: AsRef<Path>>(path: Option<P>) -> PathBuf {
    if let Some(p) = path {
        return p.as_ref().to_path_buf();
    }
    let mut current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if current_dir.join(".git").is_dir() {
            return current_dir;
        }
        if !current_dir.pop() {
            break;
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// A single step in the session - basically a prompt and a patch.
#[derive(Debug, Deserialize, Serialize)]
pub struct Step {
    pub prompt: PromptInput,
    pub patch: Option<Patch>,
}

/// A serializable session, which persists between invocations.
#[derive(Debug, Deserialize, Serialize)]
pub struct Session {
    pub root: PathBuf,
    pub dialect: Dialect,
    pub model: Option<Model>,
    pub steps: Vec<Step>,
    pub context: Vec<Context>,
    editable: Vec<PathBuf>,
}

impl Session {
    /// Creates a new Context with the specified root directory and dialect.
    pub fn new(root: Option<PathBuf>, dialect: Dialect, model: Model) -> Self {
        Self {
            root: find_root(root).canonicalize().unwrap(),
            model: Some(model),
            dialect,
            steps: vec![],
            context: vec![],
            editable: vec![],
        }
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

    /// Rolls back the last patch and sets it to None, allowing for a retry.
    pub fn retry(&mut self) -> Result<()> {
        if let Some(step) = self.steps.last() {
            if let Some(patch) = &step.patch {
                self.rollback(patch)?;
            }
        }
        if let Some(step) = self.steps.last_mut() {
            step.patch = None;
        }
        Ok(())
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
    pub fn add_patch(&mut self, patch: Patch) {
        if let Some(step) = self.steps.last_mut() {
            step.patch = Some(patch);
        }
    }

    /// Adds a new prompt to the session.
    pub fn add_prompt(&mut self, prompt: PromptInput) {
        self.steps.push(Step {
            prompt,
            patch: None,
        });
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
        let path = path.as_ref();
        if path.is_relative() {
            if let Ok(current_dir) = env::current_dir() {
                let absolute_path = current_dir
                    .join(path)
                    .canonicalize()
                    .map_err(|e| TenxError::fio(e, path))?;

                Ok(absolute_path
                    .strip_prefix(&self.root)
                    .unwrap_or(&absolute_path)
                    .to_path_buf())
            } else {
                Ok(self.root.join(path))
            }
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

    pub fn apply_patch(&mut self, patch: &Patch) -> Result<()> {
        for change in &patch.changes {
            match change {
                Change::Replace(replace) => {
                    let path = self.abspath(&replace.path)?;
                    let current_content =
                        fs::read_to_string(&path).map_err(|e| TenxError::fio(e, path.clone()))?;
                    let new_content = replace.apply(&current_content)?;
                    fs::write(&path, &new_content).map_err(|e| TenxError::fio(e, path.clone()))?;
                }
                Change::Write(write_file) => {
                    fs::write(self.abspath(&write_file.path)?, &write_file.content)
                        .map_err(|e| TenxError::fio(e, write_file.path.clone()))?;
                }
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::TempEnv;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_add_context_ignores_duplicates() {
        let mut session = Session::new(
            None,
            Dialect::Tags(crate::dialect::Tags {}),
            Model::Dummy(crate::model::Dummy::default()),
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
    fn test_add_path() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let working_dir = temp_dir.path().join("working");
        fs::create_dir(&working_dir).unwrap();
        let sub_dir = working_dir.join("subdir");
        fs::create_dir(&sub_dir).unwrap();

        let mut session = Session::new(
            Some(working_dir.clone()),
            Dialect::Tags(crate::dialect::Tags {}),
            Model::Dummy(crate::model::Dummy::default()),
        );

        // Test 1: Current dir is the working directory
        {
            let _temp_env = TempEnv::new(&working_dir).unwrap();
            fs::File::create(working_dir.join("file.txt")).unwrap();
            session.add_ctx_path("file.txt")?;
            assert_eq!(session.context.last().unwrap().name, "file.txt");
        }

        // Test 2: Current dir is under the working directory
        {
            let _temp_env = TempEnv::new(&sub_dir).unwrap();
            fs::File::create(sub_dir.join("subfile.txt")).unwrap();
            session.add_ctx_path("subfile.txt")?;
            assert_eq!(session.context.last().unwrap().name, "subdir/subfile.txt");
        }

        // Test 3: Current dir is outside the working directory
        {
            let outside_dir = temp_dir.path().join("outside");
            fs::create_dir(&outside_dir).unwrap();
            let _temp_env = TempEnv::new(&outside_dir).unwrap();
            fs::File::create(outside_dir.join("outsidefile.txt")).unwrap();
            session.add_ctx_path("outsidefile.txt")?;
            assert_eq!(
                session.context.last().unwrap().name,
                outside_dir
                    .join("outsidefile.txt")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
            );
        }

        Ok(())
    }
}
