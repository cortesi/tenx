use std::{
    env,
    path::{Path, PathBuf},
};

use colored::*;
use libruskel::Ruskel;
use serde::{Deserialize, Serialize};

use crate::{dialect::Dialect, model::Model, prompt::PromptInput, Result, TenxError};

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
    pub fn body(&self) -> Result<String> {
        match &self.data {
            ContextData::String(content) => Ok(content.clone()),
            ContextData::Path(path) => Ok(std::fs::read_to_string(path).map_err(TenxError::Io)?),
        }
    }
}

/// Finds the working directory based on the given path or git repo root.
pub fn find_working_dir<P: AsRef<Path>>(path: Option<P>) -> PathBuf {
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

/// The serializable state of Tenx, which persists between invocations.
#[derive(Debug, Deserialize, Serialize)]
pub struct Session {
    pub working_directory: PathBuf,
    pub dialect: Dialect,
    pub model: Option<Model>,
    pub prompt_inputs: Vec<PromptInput>,
    pub context: Vec<Context>,
    pub editable: Vec<PathBuf>,
}

impl Session {
    /// Creates a new Context with the specified working directory and dialect.
    pub fn new(working_directory: Option<PathBuf>, dialect: Dialect, model: Model) -> Self {
        Self {
            working_directory: find_working_dir(working_directory).canonicalize().unwrap(),
            model: Some(model),
            dialect,
            prompt_inputs: vec![],
            context: vec![],
            editable: vec![],
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

    /// Normalizes a path relative to the working directory.
    fn normalize_path<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        let path = path.as_ref();
        if path.is_relative() {
            if let Ok(current_dir) = env::current_dir() {
                let absolute_path = current_dir
                    .join(path)
                    .canonicalize()
                    .map_err(TenxError::Io)?;
                Ok(absolute_path
                    .strip_prefix(&self.working_directory)
                    .unwrap_or(&absolute_path)
                    .to_path_buf())
            } else {
                Ok(self.working_directory.join(path))
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

    /// Pretty prints the Session information.
    pub fn pretty_print(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "{} {:?}\n",
            "Working Directory:".blue().bold(),
            self.working_directory
        ));
        output.push_str(&format!(
            "{} {:?}\n",
            "Dialect:".blue().bold(),
            self.dialect
        ));
        output.push_str(&format!("{}\n", "Context:".blue().bold()));
        for context in &self.context {
            output.push_str(&format!("  - {:?}: {}\n", context.ty, context.name));
        }
        output.push_str(&format!("{}\n", "Edit Paths:".blue().bold()));
        for path in self.editable.iter() {
            output.push_str(&format!("  - {}\n", path.display()));
        }

        output
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
        assert_eq!(session.context[0].body().unwrap(), "content");
    }

    #[test]
    fn test_add_path() -> Result<()> {
        let temp_dir = tempdir()?;
        let working_dir = temp_dir.path().join("working");
        fs::create_dir(&working_dir)?;
        let sub_dir = working_dir.join("subdir");
        fs::create_dir(&sub_dir)?;

        let mut session = Session::new(
            Some(working_dir.clone()),
            Dialect::Tags(crate::dialect::Tags {}),
            Model::Dummy(crate::model::Dummy::default()),
        );

        // Test 1: Current dir is the working directory
        {
            let _temp_env = TempEnv::new(&working_dir)?;
            fs::File::create(working_dir.join("file.txt"))?;
            session.add_ctx_path("file.txt")?;
            assert_eq!(session.context.last().unwrap().name, "file.txt");
        }

        // Test 2: Current dir is under the working directory
        {
            let _temp_env = TempEnv::new(&sub_dir)?;
            fs::File::create(sub_dir.join("subfile.txt"))?;
            session.add_ctx_path("subfile.txt")?;
            assert_eq!(session.context.last().unwrap().name, "subdir/subfile.txt");
        }

        // Test 3: Current dir is outside the working directory
        {
            let outside_dir = temp_dir.path().join("outside");
            fs::create_dir(&outside_dir)?;
            let _temp_env = TempEnv::new(&outside_dir)?;
            fs::File::create(outside_dir.join("outsidefile.txt"))?;
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

