use anyhow::{Context as AnyhowContext, Result};
use std::{fs, io::Write, process::Command};
use tempfile::NamedTempFile;

use libtenx::prompt::Prompt;
use libtenx::Session;

/// Returns the user's preferred editor.
fn get_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
}

/// Renders a step as a comment.
fn render_step_commented(session: &libtenx::Session, step_offset: usize) -> String {
    let mut text = String::new();
    let steps = session.steps();
    let step = &steps[step_offset];

    text.push_str(&format!("# Step {}\n", step_offset));
    text.push_str("# ====\n#\n");
    text.push_str("# Prompt:\n# -------\n");
    for line in step.prompt.text().lines() {
        text.push_str(&format!("# {}\n", line));
    }
    if let Some(response) = &step.model_response {
        if let Some(patch) = &response.patch {
            if let Some(comment) = &patch.comment {
                text.push_str("#\n# Response:\n# ---------\n");
                for line in comment.lines() {
                    text.push_str(&format!("# {}\n", line));
                }
            }
        }
    }
    text.push('\n');
    text
}

/// Renders the initial text for the user to edit.
fn render_initial_text(session: &libtenx::Session) -> String {
    let mut text = String::new();
    let steps = session.steps();

    if let Some(last_step) = steps.last() {
        // Render the most recent prompt as editable
        text.push_str(last_step.prompt.text());
        text.push_str("\n\n");
    }

    // Add previous steps as comments
    for i in (0..steps.len().saturating_sub(1)).rev() {
        text.push_str(&render_step_commented(session, i));
        if i == 0 {
            text.push('\n');
        }
    }

    text
}

/// Parses the edited text into a Prompt.
fn parse_edited_text(input: &str) -> Prompt {
    let mut user_prompt = String::new();

    for line in input.lines() {
        if !line.trim().starts_with('#') && !line.trim().is_empty() {
            user_prompt.push_str(line);
            user_prompt.push('\n');
        }
    }

    Prompt::User(user_prompt.trim().to_string())
}

/// Opens an editor for the user to input their prompt.
pub fn edit_prompt(session: &Session) -> Result<Option<Prompt>> {
    let mut temp_file = NamedTempFile::new()?;
    let initial_text = render_initial_text(session);
    temp_file.write_all(initial_text.as_bytes())?;
    temp_file.flush()?;
    let initial_metadata = temp_file.as_file().metadata()?;
    let editor = get_editor();
    Command::new(editor)
        .arg(temp_file.path())
        .status()
        .context("Failed to open editor")?;
    let final_metadata = temp_file.as_file().metadata()?;
    if final_metadata.modified()? > initial_metadata.modified()? {
        let edited_content =
            fs::read_to_string(temp_file.path()).context("Failed to read temporary file")?;
        let prompt = parse_edited_text(&edited_content);
        Ok(Some(prompt))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use libtenx::patch::Patch;
    use pretty_assertions::assert_eq;

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
        assert_eq!(prompt.text(), "New prompt here\nwith multiple lines");
    }

    #[test]
    fn test_render_initial_text() {
        let mut session = Session::default();
        session
            .add_prompt(Prompt::User(
                "First prompt\nwith multiple lines".to_string(),
            ))
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.model_response = Some(libtenx::ModelResponse {
                patch: Some(Patch {
                    changes: vec![],
                    comment: Some("First response\nalso with multiple lines".to_string()),
                }),
                operations: vec![],
                usage: None,
            });
        }

        // Test editing first step (retry case)
        let rendered_step_0 = render_initial_text(&session);
        assert_eq!(rendered_step_0, "First prompt\nwith multiple lines\n\n");

        // Test adding new step (edit case)
        let rendered_new_step = render_initial_text(&session);
        assert_eq!(
            rendered_new_step,
            indoc! {"
            First prompt
            with multiple lines

        "}
        );

        // Add second step
        session
            .add_prompt(Prompt::User(
                "Second prompt\nstill multiple lines".to_string(),
            ))
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.model_response = Some(libtenx::ModelResponse {
                patch: Some(Patch {
                    changes: vec![],
                    comment: Some("Second response\nyet more lines".to_string()),
                }),
                operations: vec![],
                usage: None,
            });
        }

        // Test editing second step
        let rendered_step_1 = render_initial_text(&session);
        assert_eq!(
            rendered_step_1,
            indoc! {"
            Second prompt
            still multiple lines

            # Step 0
            # ====
            #
            # Prompt:
            # -------
            # First prompt
            # with multiple lines
            #
            # Response:
            # ---------
            # First response
            # also with multiple lines


        "}
        );
    }
}
