use anyhow::{Context as AnyhowContext, Result};
use std::{fs, io::Write, path::PathBuf, process::Command};
use tempfile::NamedTempFile;

use libtenx::Prompt;

/// Returns the user's preferred editor.
fn get_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
}

/// Renders the initial text for the user to edit.
fn render_initial_text(files: &[PathBuf], attach: &[PathBuf]) -> String {
    let mut text = String::from("\n# Editable files:\n");
    for file in files {
        text.push_str(&format!("# - {}\n", file.display()));
    }
    if !attach.is_empty() {
        text.push_str("#\n# Context files:\n");
        for file in attach {
            text.push_str(&format!("# - {}\n", file.display()));
        }
    }
    text.push_str("#\n");
    text
}

/// Parses the edited text into a Prompt.
fn parse_edited_text(input: &str) -> Prompt {
    let user_prompt = input
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<&str>>()
        .join("\n")
        .trim()
        .to_string();
    Prompt {
        attach_paths: vec![],
        edit_paths: vec![],
        user_prompt,
    }
}

/// Opens an editor for the user to input their prompt.
pub fn edit_prompt(files: &[PathBuf], attach: &[PathBuf]) -> Result<Prompt> {
    let mut temp_file = NamedTempFile::new()?;
    let initial_text = render_initial_text(files, attach);
    temp_file.write_all(initial_text.as_bytes())?;
    temp_file.flush()?;
    let editor = get_editor();
    Command::new(editor)
        .arg(temp_file.path())
        .status()
        .context("Failed to open editor")?;
    let edited_content =
        fs::read_to_string(temp_file.path()).context("Failed to read temporary file")?;
    Ok(parse_edited_text(&edited_content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edited_text() {
        let input = r#"
This is a user prompt
with multiple lines.

# Editable files:
# - src/main.rs
#
# Context files:
# - src/lib.rs
#
"#;
        let prompt = parse_edited_text(input);
        assert_eq!(
            prompt.user_prompt,
            "This is a user prompt\nwith multiple lines."
        );
        assert!(prompt.attach_paths.is_empty());
        assert!(prompt.edit_paths.is_empty());
    }
}
