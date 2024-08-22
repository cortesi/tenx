use crate::{dialect::DialectProvider, patch::Patch, prompt::PromptInput, Result, Session};
use std::path::PathBuf;

/// A dummy dialect for testing purposes.
pub struct DummyDialect {
    parse_result: Result<Patch>,
}

impl DummyDialect {
    /// Creates a new DummyDialect with a specified parse result.
    pub fn new(parse_result: Result<Patch>) -> Self {
        Self { parse_result }
    }
}

impl DialectProvider for DummyDialect {
    fn name(&self) -> &'static str {
        "dummy"
    }

    fn system(&self) -> String {
        String::new()
    }

    fn render_prompt(&self, _p: &PromptInput) -> Result<String> {
        Ok(String::new())
    }

    fn render_editables(&self, _paths: Vec<PathBuf>) -> Result<String> {
        Ok(String::new())
    }

    fn render_context(&self, _p: &Session) -> Result<String> {
        Ok(String::new())
    }

    fn render_patch(&self, _patch: &Patch) -> Result<String> {
        Ok(String::new())
    }

    fn parse(&self, _txt: &str) -> Result<Patch> {
        self.parse_result.clone()
    }
}

