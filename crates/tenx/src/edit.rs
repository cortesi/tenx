use anyhow::{Context as AnyhowContext, Result};
use std::{fs, io::Write, path::PathBuf, process::Command};
use tempfile::NamedTempFile;

use indoc::indoc;
use libtenx::PromptInput;

const EDITABLE_FILES_HEADING: &str = "### Editable files:";
const CONTEXT_FILES_HEADING: &str = "### Context files:";

/// Returns the user's preferred editor.
fn get_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
}

/// Renders the initial text for the user to edit.
fn render_initial_text(files: &[PathBuf]) -> String {
    let mut text = String::from(indoc! {r#"
            
            # Enter your prompt above. You may edit the file lists below."

        "#
    });
    text.push_str(&format!("{}\n", EDITABLE_FILES_HEADING));
    for file in files {
        text.push_str(&format!("{}\n", file.display()));
    }
    text.push('\n');
    text
}

/// Parses the edited text into a Prompt.
fn parse_edited_text(input: &str) -> PromptInput {
    let lines = input.lines();
    let mut user_prompt = String::new();
    let mut edit_paths = Vec::new();
    let mut attach_paths = Vec::new();
    let mut current_section = None;

    for line in lines {
        let trimmed = line.trim();
        if trimmed == EDITABLE_FILES_HEADING {
            current_section = Some("editable");
        } else if trimmed == CONTEXT_FILES_HEADING {
            current_section = Some("context");
        } else if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        } else {
            match current_section {
                Some("editable") => edit_paths.push(PathBuf::from(trimmed)),
                Some("context") => attach_paths.push(PathBuf::from(trimmed)),
                _ => user_prompt.push_str(&format!("{}\n", line)),
            }
        }
    }

    PromptInput {
        edit_paths,
        user_prompt: user_prompt.trim().to_string(),
    }
}

/// Opens an editor for the user to input their prompt.
pub fn edit_prompt(files: &[PathBuf]) -> Result<Option<PromptInput>> {
    let mut temp_file = NamedTempFile::new()?;
    let initial_text = render_initial_text(files);
    temp_file.write_all(initial_text.as_bytes())?;
    temp_file.flush()?;
    let editor = get_editor();
    Command::new(editor)
        .arg(temp_file.path())
        .status()
        .context("Failed to open editor")?;
    let edited_content =
        fs::read_to_string(temp_file.path()).context("Failed to read temporary file")?;
    let prompt = parse_edited_text(&edited_content);
    if prompt.user_prompt.is_empty() {
        Ok(None)
    } else {
        Ok(Some(prompt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edited_text() {
        let input = format!(
            indoc! {r#"
                This is a user prompt
                with multiple lines.

                {}
                src/main.rs
                src/lib.rs

                {}
                tests/test_main.rs
                README.md
            "#
            },
            EDITABLE_FILES_HEADING, CONTEXT_FILES_HEADING
        );
        let prompt = parse_edited_text(&input);
        assert_eq!(
            prompt.user_prompt,
            "This is a user prompt\nwith multiple lines."
        );
        assert_eq!(
            prompt.edit_paths,
            vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")]
        );
    }
}
