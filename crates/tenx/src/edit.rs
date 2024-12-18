use anyhow::{Context as AnyhowContext, Result};
use std::{fs, io::Write, process::Command};
use tempfile::NamedTempFile;
use tokio::sync::mpsc;

use libtenx::{events::Event, session::Session};

/// Returns the user's preferred editor.
fn get_editor() -> (String, Vec<String>) {
    let editor_str = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    let mut parts = editor_str.split_whitespace();
    let command = parts.next().unwrap_or("vim").to_string();
    let args: Vec<String> = parts.map(|s| s.to_string()).collect();
    (command, args)
}

/// Renders a step as a comment.
fn render_step_commented(session: &Session, step_offset: usize) -> String {
    let mut text = String::new();
    let steps = session.steps();
    let step = &steps[step_offset];

    text.push_str(&format!("# Step {}\n", step_offset));
    text.push_str("# ====\n#\n");
    text.push_str("# Prompt:\n# -------\n");
    for line in step.prompt.lines() {
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
fn render_initial_text(session: &Session, retry: bool) -> Result<String> {
    let mut text = String::new();
    let steps = session.steps();

    if retry {
        if steps.is_empty() {
            anyhow::bail!("Cannot retry without at least one step");
        }
        let last = steps.last().unwrap();
        text.push_str(&last.prompt);
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
pub fn edit_prompt(
    session: &Session,
    retry: bool,
    event_sender: &Option<mpsc::Sender<Event>>,
) -> Result<Option<String>> {
    if let Some(sender) = event_sender {
        let _ = sender.try_send(Event::Interact);
    }
    let mut temp_file = NamedTempFile::new()?;
    let initial_text = render_initial_text(session, retry)?;
    temp_file.write_all(initial_text.as_bytes())?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;

    let initial_content = fs::read_to_string(temp_file.path())?;

    let (editor, args) = get_editor();
    let mut cmd = Command::new(editor);
    cmd.args(args);
    cmd.arg(temp_file.path());
    let _status = cmd.status().context("Failed to open editor")?;

    // Re-read the file after editing
    let edited_content = fs::read_to_string(temp_file.path())?;
    if edited_content != initial_content {
        let prompt = parse_edited_text(&edited_content);
        Ok(Some(prompt))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use libtenx::{
        patch::Patch,
        session::{ModelResponse, StepType},
    };

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
            .add_prompt(
                "test_model".into(),
                "First prompt\nwith multiple lines".to_string(),
                StepType::Code,
            )
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.model_response = Some(ModelResponse {
                patch: Some(Patch { changes: vec![] }),
                operations: vec![],
                usage: None,
                comment: Some("First response\nalso with multiple lines".to_string()),
                response_text: Some("First response\nalso with multiple lines".to_string()),
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
            .add_prompt(
                "test_model".into(),
                "Second prompt\nstill multiple lines".to_string(),
                StepType::Code,
            )
            .unwrap();
        if let Some(step) = session.last_step_mut() {
            step.model_response = Some(ModelResponse {
                patch: Some(Patch { changes: vec![] }),
                operations: vec![],
                usage: None,
                comment: Some("Second response\nyet more lines".to_string()),
                response_text: Some("Second response\nyet more lines".to_string()),
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
