use anyhow::{Context as AnyhowContext, Result};
use std::{fs, io::Write, process::Command};
use tempfile::NamedTempFile;

use libtenx::prompt::PromptInput;
use libtenx::Session;

/// Returns the user's preferred editor.
fn get_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
}

/// Renders the initial text for the user to edit.
fn render_initial_text(session: &libtenx::Session, step_offset: usize) -> String {
    let mut text = String::new();
    text.push('\n'); // Space for new prompt

    for (i, step) in session.steps()[..=step_offset].iter().rev().enumerate() {
        text.push_str(&format!("# Step {}\n", step_offset + 1 - i));
        text.push_str("# ====\n\n");
        text.push_str("# Prompt:\n# -------\n");
        for line in step.prompt.user_prompt.lines() {
            text.push_str(&format!("# {}\n", line));
        }
        if let Some(patch) = &step.patch {
            if let Some(comment) = &patch.comment {
                text.push_str("\n# Response:\n# ---------\n");
                for line in comment.lines() {
                    text.push_str(&format!("# {}\n", line));
                }
            }
        }
        text.push_str("\n\n");
    }

    text
}

/// Parses the edited text into a Prompt.
fn parse_edited_text(input: &str) -> PromptInput {
    let mut user_prompt = String::new();

    for line in input.lines() {
        if !line.trim().starts_with('#') && !line.trim().is_empty() {
            user_prompt.push_str(line);
            user_prompt.push('\n');
        }
    }

    PromptInput {
        user_prompt: user_prompt.trim().to_string(),
    }
}

/// Opens an editor for the user to input their prompt.
pub fn edit_prompt(session: &Session) -> Result<Option<PromptInput>> {
    edit_prompt_at_step(session, session.steps().len() - 1)
}

/// Opens an editor for the user to input their prompt at a specific step.
pub fn edit_prompt_at_step(session: &Session, step_offset: usize) -> Result<Option<PromptInput>> {
    let mut temp_file = NamedTempFile::new()?;
    let initial_text = render_initial_text(session, step_offset);
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
    use indoc::indoc;
    use libtenx::patch::Patch;
    use std::path::PathBuf;

    #[test]
    fn test_parse_edited_text() {
        let input = indoc! {"
            New prompt here
            with multiple lines

            # Step 2
            # Prompt:
            # Previous prompt
            # with multiple lines
            # Response:
            # Previous response
            # also with multiple lines

            # Step 1
            # Prompt:
            # First prompt
            # Response:
            # First response
        "};
        let prompt = parse_edited_text(input);
        assert_eq!(prompt.user_prompt, "New prompt here\nwith multiple lines");
    }

    #[test]
    fn test_render_initial_text() {
        let mut session = Session::new(
            PathBuf::from("/"),
            libtenx::dialect::Dialect::Dummy(libtenx::dialect::DummyDialect::default()),
            libtenx::model::Model::Dummy(libtenx::model::DummyModel::default()),
        );
        session
            .add_prompt(PromptInput {
                user_prompt: indoc! {"
                    First prompt
                    with multiple lines
                "}
                .to_string(),
            })
            .unwrap();
        session.set_last_patch(&Patch {
            changes: vec![],
            comment: Some(
                indoc! {"
                First response
                also with multiple lines
            "}
                .to_string(),
            ),
            cache: Default::default(),
        });
        session
            .add_prompt(PromptInput {
                user_prompt: indoc! {"
                    Second prompt
                    still multiple lines
                "}
                .to_string(),
            })
            .unwrap();
        session.set_last_patch(&Patch {
            changes: vec![],
            comment: Some(
                indoc! {"
                Second response
                yet more lines
            "}
                .to_string(),
            ),
            cache: Default::default(),
        });

        let rendered = render_initial_text(&session, 1);
        assert!(rendered.contains(indoc! {"
            # Step 2
            # ====

            # Prompt:
            # -------
            # Second prompt
            # still multiple lines

            # Response:
            # ---------
            # Second response
            # yet more lines
        "}));
        assert!(rendered.contains(indoc! {"
            # Step 1
            # ====

            # Prompt:
            # -------
            # First prompt
            # with multiple lines

            # Response:
            # ---------
            # First response
            # also with multiple lines
        "}));
        assert!(!rendered.contains("# Step 3"));
    }
}
