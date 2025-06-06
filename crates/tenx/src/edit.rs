use std::{fs, io::Write, process::Command};

use anyhow::{Context as AnyhowContext, Result};
use tempfile::NamedTempFile;
use tokio::sync::mpsc;

use libtenx::{config::Config, events::Event, session::Session};

const SESSION_INFO_MARKER: &str = "\n** Only edit prompt text ABOVE this marker. **\n";

const SESSION_HEADER: &str =
    "______________________________________________\n\n# Session Summary\n\n";

/// Returns the user's preferred editor.
fn get_editor() -> (String, Vec<String>) {
    let editor_str = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    let mut parts = editor_str.split_whitespace();
    let command = parts.next().unwrap_or("vim").to_string();
    let args: Vec<String> = parts.map(|s| s.to_string()).collect();
    (command, args)
}

/// Renders the session summary
fn render_session_summary(session: &Session, _retry: bool) -> String {
    let mut md = unirend::Markdown::new();
    session
        .render(&Config::default(), &mut md, unirend::Detail::Default)
        .unwrap();
    md.render()
}

/// Renders the text for the user to edit. This includes space for the user's prompt, and a
/// summary.
fn render_edit_text(session: &Session, retry: bool) -> Result<String> {
    let mut text = String::new();
    match session.last_step() {
        Some(step) => {
            if retry {
                text.push_str(&step.prompt);
            }
        }
        None => {
            if retry {
                anyhow::bail!("Cannot retry without at least one step");
            }
            text.push_str("Enter your prompt here...");
        }
    }

    text.push('\n');
    text.push_str(SESSION_INFO_MARKER);
    text.push_str(SESSION_HEADER);
    text.push_str(&render_session_summary(session, retry));

    Ok(text)
}

/// Parses the edited text into a Prompt.
fn parse_edited_text(input: &str) -> String {
    if let Some(marker_pos) = input.find(SESSION_INFO_MARKER) {
        input[..marker_pos].trim().to_string()
    } else {
        input.trim().to_string()
    }
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
    let mut temp_file = NamedTempFile::with_suffix(".md")?;
    let edit_text = render_edit_text(session, retry)?;
    temp_file.write_all(edit_text.as_bytes())?;
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
        session::{Action, ModelResponse, Step},
        strategy, testutils,
    };
    use state::Patch;

    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_edited_text() {
        // Basic parse test with session info
        let input = format!("New prompt{SESSION_INFO_MARKER}Session info...");
        let prompt = parse_edited_text(&input);
        assert_eq!(prompt, "New prompt");

        // Multi-line prompt test
        let input = format!("Line 1\nLine 2{SESSION_INFO_MARKER}Session info...");
        let prompt = parse_edited_text(&input);
        assert_eq!(prompt, "Line 1\nLine 2");

        // No session marker should return full text trimmed
        let input = "Just a prompt\nNo session info";
        let prompt = parse_edited_text(input);
        assert_eq!(prompt, "Just a prompt\nNo session info");
    }

    #[test]
    fn test_render_initial_text_empty_session() {
        let p = testutils::test_project();

        // Should error on retry with empty session
        assert!(render_edit_text(&p.session, true).is_err());

        // Should succeed with no retry
        let rendered = render_edit_text(&p.session, false).unwrap();
        assert!(rendered.contains(SESSION_INFO_MARKER));
    }

    #[test]
    fn test_render_and_parse_roundtrip() -> anyhow::Result<()> {
        let mut p = testutils::test_project();
        p.session
            .add_action(Action::new(
                &p.config,
                strategy::Strategy::Code(strategy::Code::default()),
            )?)
            .unwrap();

        // Add two steps with responses
        for (prompt, response) in [
            ("First prompt\nMultiline", "First response"),
            ("Second prompt", "Second response"),
        ] {
            p.session
                .last_action_mut()
                .unwrap()
                .add_step(Step::new(
                    "test_model".into(),
                    prompt.to_string(),
                    libtenx::strategy::StrategyStep::Code(libtenx::strategy::CodeStep::default()),
                ))
                .unwrap();
            if let Some(step) = p.session.last_step_mut() {
                step.model_response = Some(ModelResponse {
                    patch: Some(Patch { ops: vec![] }),
                    usage: None,
                    comment: Some(response.to_string()),
                    raw_response: Some(response.to_string()),
                });
            }
        }

        // Test retry=false (empty prompt)
        let rendered = render_edit_text(&p.session, false).unwrap();
        let parsed = parse_edited_text(&rendered);
        assert_eq!(parsed.trim(), "");

        // Test retry=true (should show last prompt)
        let rendered = render_edit_text(&p.session, true).unwrap();
        let parsed = parse_edited_text(&rendered);
        assert_eq!(parsed, "Second prompt");
        Ok(())
    }
}
