use anyhow::{Context as AnyhowContext, Result};
use std::{fs, io::Write, process::Command};
use tempfile::NamedTempFile;

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
        if let Some(comment) = &response.comment {
            text.push_str("#\n# Response:\n# ---------\n");
            for line in comment.lines() {
                text.push_str(&format!("# {}\n", line));
            }
        }
    }
    text.push('\n');
    text
}

/// Renders all steps as comments.
fn comment_all_steps(session: &Session) -> String {
    let mut text = "\n\n".to_string();
    for i in (0..session.steps().len()).rev() {
        text.push_str(&render_step_commented(session, i));
        if i == 0 {
            text.push('\n');
        }
    }
    text
}

/// Renders the initial text for the user to edit.
fn render_initial_text(session: &libtenx::Session, retry: bool) -> Result<String> {
    let mut text = String::new();
    let steps = session.steps();

    if retry {
        if steps.is_empty() {
            anyhow::bail!("Cannot retry without at least one step");
        }
        let last = steps.last().unwrap();
        text.push_str(last.prompt.text());
        text.push_str("\n\n");
        // Add all but the last step as comments
        for i in (0..steps.len() - 1).rev() {
            text.push_str(&render_step_commented(session, i));
            if i == 0 {
                text.push('\n');
            }
        }
    } else {
        text.push_str(&comment_all_steps(session));
    }

    Ok(text)
}

/// Parses the edited text into a Prompt.
fn parse_edited_text(input: &str) -> String {
    let mut user_prompt = String::new();

    for line in input.lines() {
        if !line.trim().starts_with('#') && !line.trim().is_empty() {
            user_prompt.push_str(line);
            user_prompt.push('\n');
        }
    }

    user_prompt.trim().to_string()
}

/// Opens an editor for the user to input their prompt.
pub fn edit_prompt(session: &Session, retry: bool) -> Result<Option<String>> {
    let mut temp_file = NamedTempFile::new()?;
    let initial_text = render_initial_text(session, retry)?;
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
    use libtenx::{patch::Patch, prompt::Prompt};

    use indoc::indoc;
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
        assert_eq!(prompt, "New prompt here\nwith multiple lines");
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
                patch: Some(Patch { changes: vec![] }),
                operations: vec![],
                usage: None,
                comment: Some("First response\nalso with multiple lines".to_string()),
            });
        }

        // Test rendering with retry on first step
        let rendered_text = render_initial_text(&session, true).unwrap();
        assert_eq!(rendered_text, "First prompt\nwith multiple lines\n\n");

        // Test rendering with retry on empty session should error
        let empty_session = Session::default();
        assert!(render_initial_text(&empty_session, true).is_err());

        // Add second step
        session
            .add_prompt(Prompt::User(
                "Second prompt\nstill multiple lines".to_string(),
            ))
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.model_response = Some(libtenx::ModelResponse {
                patch: Some(Patch { changes: vec![] }),
                operations: vec![],
                usage: None,
                comment: Some("Second response\nyet more lines".to_string()),
            });
        }

        // Test rendering with retry=false should comment all steps
        let rendered_no_retry = render_initial_text(&session, false).unwrap();
        assert_eq!(
            rendered_no_retry,
            indoc! {"


            # Step 1
            # ====
            #
            # Prompt:
            # -------
            # Second prompt
            # still multiple lines
            #
            # Response:
            # ---------
            # Second response
            # yet more lines

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

        // Test rendering with retry=true should show last step uncommented
        let rendered_retry = render_initial_text(&session, true).unwrap();
        assert_eq!(
            rendered_retry,
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
