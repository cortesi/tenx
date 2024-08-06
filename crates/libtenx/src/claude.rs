use crate::query::Query;
use std::error::Error;

struct Claude;

impl Claude {
    pub fn new() -> Self {
        Claude
    }

    pub async fn render(&self, query: &Query) -> Result<String, Box<dyn Error>> {
        // Here we'll implement the logic to render the query to text
        // For now, we'll just return a placeholder string
        let rendered = format!(
            "Project Root: {}\n\
             Current Directory: {}\n\
             Globs: {:?}\n\
             Prompt: {}",
            query.project_root.display(),
            query.current_directory.display(),
            query.include_globs,
            query.user_prompt
        );

        Ok(rendered)
    }
}
