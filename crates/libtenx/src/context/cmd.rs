use super::ContextItem;
use super::ContextProvider;
use crate::config::Config;
use crate::error::Result;
use crate::exec::exec;
use crate::session::Session;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A context provider that captures command output
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub(crate) command: String,
    pub(crate) content: String,
}

impl Cmd {
    pub(crate) fn new(command: String) -> Self {
        Self {
            command,
            content: String::new(),
        }
    }
}

#[async_trait]
impl ContextProvider for Cmd {
    fn context_items(&self, _config: &Config, _session: &Session) -> Result<Vec<ContextItem>> {
        Ok(vec![ContextItem {
            ty: "cmd".to_string(),
            source: self.command.clone(),
            body: self.content.clone(),
        }])
    }

    fn human(&self) -> String {
        format!("cmd: {}", self.command)
    }

    fn id(&self) -> String {
        self.command.clone()
    }

    async fn refresh(&mut self, config: &Config) -> Result<()> {
        let (_, stdout, stderr) = exec(config.project_root(), &self.command)?;

        let mut content = String::new();
        let stdout = stdout.trim_end();
        if !stdout.is_empty() {
            content.push_str(stdout);
        }
        let stderr = stderr.trim_end();
        if !stderr.is_empty() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(stderr);
        }
        self.content = content;
        Ok(())
    }

    async fn needs_refresh(&self, _config: &Config) -> bool {
        self.content.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::{Context, ContextProvider},
        testutils::test_project,
    };
    use tokio::runtime::Runtime;

    #[test]
    fn test_cmd_context() {
        let rt = Runtime::new().unwrap();
        let test_project = test_project();
        test_project.create_file_tree(&["test.txt"]);
        let config = test_project.config;
        let session = Session::new(&config).unwrap();
        let cmd = "echo 'hello world' && echo 'error' >&2";
        let mut context = Context::new_cmd(cmd);

        // Initial state
        assert!(rt.block_on(async { context.needs_refresh(&config).await }));

        // After refresh
        rt.block_on(async { context.refresh(&config).await.unwrap() });

        let items = rt.block_on(async { context.context_items(&config, &session).unwrap() });
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].ty, "cmd");
        assert_eq!(items[0].source, cmd);
        assert_eq!(items[0].body, "hello world\nerror");

        assert_eq!(context.human(), format!("cmd: {cmd}"));
        assert!(!rt.block_on(async { context.needs_refresh(&config).await }));
    }
}
