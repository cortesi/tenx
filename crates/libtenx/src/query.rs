use std::collections::HashSet;
use std::path::PathBuf;

pub struct Query {
    pub project_root: PathBuf,
    pub current_directory: PathBuf,
    pub include_globs: HashSet<String>,
    pub user_prompt: String,
}

impl Query {
    pub fn new(project_root: PathBuf, current_directory: PathBuf) -> Self {
        Query {
            project_root,
            current_directory,
            include_globs: HashSet::new(),
            user_prompt: String::new(),
        }
    }

    pub fn with_glob(mut self, glob: &str) -> Self {
        self.include_globs.insert(glob.to_string());
        self
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.user_prompt = prompt.to_string();
        self
    }
}
