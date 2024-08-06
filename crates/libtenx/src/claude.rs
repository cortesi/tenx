use crate::error::ClaudeError;
use crate::query::Query;

#[derive(Debug, Default)]
pub struct Claude;

impl Claude {
    pub fn new() -> Self {
        Claude
    }

    pub async fn render(&self, query: &Query) -> Result<String, ClaudeError> {
        // Here we'll implement the logic to render the query to text
        // For now, we'll just return a placeholder string
        let rendered = format!(
            "
                Current Directory: {}\n\
                Globs: {:?}\n\
                Prompt: {}
            ",
            query.working_directory.display(),
            query.edit_paths,
            query.user_prompt
        );

        // Example of using our error type
        if rendered.is_empty() {
            return Err(ClaudeError::RenderError(
                "Failed to render query".to_string(),
            ));
        }

        Ok(rendered)
    }
}
