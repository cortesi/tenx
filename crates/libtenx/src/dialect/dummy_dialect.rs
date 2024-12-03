use crate::{
    config::Config, dialect::DialectProvider, session::ModelResponse, session::Session, Result,
};
use std::path::PathBuf;

/// A dummy dialect for testing purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DummyDialect {
    parse_results: Vec<Result<ModelResponse>>,
    current_index: usize,
}

impl DummyDialect {
    /// Creates a new DummyDialect with specified parse results.
    pub fn new(parse_results: Vec<Result<ModelResponse>>) -> Self {
        Self {
            parse_results,
            current_index: 0,
        }
    }
}

impl Default for DummyDialect {
    fn default() -> Self {
        Self {
            parse_results: vec![Ok(ModelResponse::default())],
            current_index: 0,
        }
    }
}

impl DialectProvider for DummyDialect {
    fn name(&self) -> &'static str {
        "dummy"
    }

    fn system(&self) -> String {
        String::new()
    }

    fn render_step_request(
        &self,
        _config: &Config,
        _session: &Session,
        _offset: usize,
    ) -> Result<String> {
        Ok(String::new())
    }

    fn render_editables(
        &self,
        _config: &Config,
        _session: &Session,
        _paths: Vec<PathBuf>,
    ) -> Result<String> {
        Ok(String::new())
    }

    fn render_context(&self, _config: &Config, _p: &Session) -> Result<String> {
        Ok(String::new())
    }

    fn render_step_response(
        &self,
        _config: &Config,
        _session: &Session,
        _offset: usize,
    ) -> Result<String> {
        Ok(String::new())
    }

    fn parse(&self, _txt: &str) -> Result<ModelResponse> {
        if self.current_index < self.parse_results.len() {
            let result = self.parse_results[self.current_index].clone();
            Ok(result?)
        } else {
            panic!("No more parse results available");
        }
    }
}
