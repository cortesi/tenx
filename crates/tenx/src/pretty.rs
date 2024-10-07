use colored::*;
use libtenx::{
    config::Config, context::ContextProvider, prompt::Prompt, Result, Session, TenxError,
};
use terminal_size::{terminal_size, Width};
use textwrap::{wrap, Options};

const DEFAULT_WIDTH: usize = 80;
const INDENT: &str = "  ";

fn format_usage(usage: &libtenx::model::Usage) -> String {
    let values = usage.values();
    let mut keys: Vec<_> = values.keys().collect();
    keys.sort();
    keys.iter()
        .map(|k| format!("{}: {}", k.blue().bold(), values.get(*k).unwrap()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pretty prints the Session information.
pub fn session(config: &Config, session: &Session, full: bool) -> Result<String> {
    let width = terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(DEFAULT_WIDTH);
    let mut output = String::new();
    output.push_str(&print_session_info(config, session));
    output.push_str(&print_context_specs(session));
    output.push_str(&print_editables(config, session)?);
    output.push_str(&print_steps(config, session, full, width)?);
    Ok(output)
}

fn print_session_info(config: &Config, _: &Session) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{} {}\n",
        "root:".blue().bold(),
        config.project_root().display()
    ));
    output
}

fn print_context_specs(session: &Session) -> String {
    let mut output = String::new();
    if !session.context().is_empty() {
        output.push_str(&format!("{}\n", "context:".blue().bold()));
        for context in session.context() {
            output.push_str(&format!("{}- {}\n", INDENT, context.human()));
        }
    }
    output
}

fn print_editables(config: &Config, session: &Session) -> Result<String> {
    let mut output = String::new();
    let editables = session.abs_editables(config)?;
    if !editables.is_empty() {
        output.push_str(&format!("{}\n", "edit:".blue().bold()));
        for path in editables {
            output.push_str(&format!(
                "{}- {}\n",
                INDENT,
                config.relpath(&path).display()
            ));
        }
    }
    Ok(output)
}

fn print_steps(config: &Config, session: &Session, full: bool, width: usize) -> Result<String> {
    if session.steps().is_empty() {
        return Ok(String::new());
    }
    let mut output = String::new();
    for (i, step) in session.steps().iter().enumerate() {
        output.push_str(&format!("\n{}\n", "=".repeat(width)));
        output.push_str(&format!("{}\n", format!("Step {}", i).cyan().bold()));
        output.push_str(&format!("{}\n", "=".repeat(width)));
        output.push_str(&render_step_prompt(step, width, full));
        output.push('\n');
        if let Some(patch) = &step.patch {
            output.push_str(&print_patch(config, patch, full, width));
        }
        if let Some(err) = &step.err {
            output.push_str(&format!(
                "{}{}\n",
                INDENT.repeat(2),
                "error:".yellow().bold()
            ));
            let error_text = if full {
                full_error(err)
            } else {
                format!("{}", err)
            };
            output.push_str(&wrapped_block(&error_text, width, INDENT.len() * 3));
            output.push('\n');
        }
        if let Some(usage) = &step.usage {
            output.push_str(&format!("{}{}\n", INDENT.repeat(2), "usage:".blue().bold()));
            for line in format_usage(usage).lines() {
                output.push_str(&format!("{}{}\n", INDENT.repeat(3), line));
            }
        }
    }
    Ok(output)
}

fn render_step_prompt(step: &libtenx::Step, width: usize, full: bool) -> String {
    let prompt_header = format!("{}{}\n", INDENT.repeat(2), "prompt:".blue().bold());
    match &step.prompt {
        Prompt::User(text) => format!(
            "{}{}",
            prompt_header,
            wrapped_block(text, width, INDENT.len() * 3)
        ),
        Prompt::Auto(text) if full => format!(
            "{}{}",
            prompt_header,
            wrapped_block(text, width, INDENT.len() * 3)
        ),
        Prompt::Auto(text) => {
            let lines: Vec<&str> = text.lines().collect();
            let first_line = lines.first().unwrap_or(&"");
            let remaining_lines = lines.len().saturating_sub(1);
            format!(
                "{}{}\n{}",
                prompt_header,
                wrapped_block(first_line, width, INDENT.len() * 3),
                wrapped_block(
                    &format!("... {} more lines", remaining_lines),
                    width,
                    INDENT.len() * 3
                )
            )
        }
    }
}

fn print_patch(config: &Config, patch: &libtenx::patch::Patch, full: bool, width: usize) -> String {
    let mut output = String::new();
    if let Some(comment) = &patch.comment {
        output.push_str(&format!(
            "{}{}\n",
            INDENT.repeat(2),
            "comment:".blue().bold()
        ));
        let comment_text = if full {
            comment.clone()
        } else {
            comment.lines().next().unwrap_or("").to_string()
        };
        output.push_str(&wrapped_block(&comment_text, width, INDENT.len() * 3));
        output.push('\n');
    }
    output.push_str(&format!(
        "{}{}\n",
        INDENT.repeat(2),
        "modified:".blue().bold()
    ));
    for change in &patch.changes {
        match change {
            libtenx::patch::Change::Write(w) => {
                let file_path = config.relpath(&w.path).display().to_string().green().bold();
                output.push_str(&format!("{}- {} (write)\n", INDENT.repeat(3), file_path));
                if full {
                    output.push_str(&wrapped_block(&w.content, width, INDENT.len() * 4));
                    output.push('\n');
                }
            }
            libtenx::patch::Change::UDiff(w) => {
                output.push_str(&format!("{} udiff \n", INDENT.repeat(3)));
                if full {
                    output.push_str(&wrapped_block(&w.patch, width, INDENT.len() * 4));
                    output.push('\n');
                }
            }
            libtenx::patch::Change::Replace(r) => {
                let file_path = config.relpath(&r.path).display().to_string().green().bold();
                output.push_str(&format!("{}- {} (replace)\n", INDENT.repeat(3), file_path));
                if full {
                    output.push_str(&format!("{}{}\n", INDENT.repeat(4), "old:".yellow().bold()));
                    output.push_str(&wrapped_block(&r.old, width, INDENT.len() * 5));
                    output.push_str(&format!(
                        "\n{}{}\n",
                        INDENT.repeat(4),
                        "new:".green().bold()
                    ));
                    output.push_str(&wrapped_block(&r.new, width, INDENT.len() * 5));
                    output.push('\n');
                }
            }
            libtenx::patch::Change::Smart(s) => {
                let file_path = config.relpath(&s.path).display().to_string().green().bold();
                output.push_str(&format!("{}- {} (smart)\n", INDENT.repeat(3), file_path));
                if full {
                    output.push_str(&wrapped_block(&s.text, width, INDENT.len() * 4));
                    output.push('\n');
                }
            }
        }
    }
    output
}

/// Pretty prints a TenxError with full details.
pub fn full_error(error: &TenxError) -> String {
    match error {
        TenxError::Validation { name, user, model } => {
            format!(
                "{}: {}\n{}: {}\n{}: {}",
                "Validation Error".red().bold(),
                name,
                "User Message".yellow().bold(),
                user,
                "Model Message".yellow().bold(),
                model
            )
        }
        TenxError::Patch { user, model } => {
            format!(
                "{}\n{}: {}\n{}: {}",
                "Patch Error".red().bold(),
                "User Message".yellow().bold(),
                user,
                "Model Message".yellow().bold(),
                model
            )
        }
        _ => format!("{:?}", error),
    }
}

fn wrapped_block(text: &str, width: usize, indent: usize) -> String {
    let ident = " ".repeat(indent);
    let options = Options::new(width - indent)
        .initial_indent(&ident)
        .subsequent_indent(&ident);
    wrap(text, &options).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use libtenx::{context::ContextSpec, patch::Patch, prompt::Prompt, Step, TenxError};
    use tempfile::TempDir;

    fn create_test_session() -> (TempDir, Session) {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();
        let mut session = Session::default();
        session
            .add_prompt(Prompt::User("Test prompt".to_string()))
            .unwrap();
        let test_file_path = root_path.join("test_file.rs");
        std::fs::write(&test_file_path, "Test content").unwrap();
        session.add_context(ContextSpec::new_glob("test_file.rs".to_string()));
        (temp_dir, session)
    }

    #[test]
    fn test_print_steps_empty_session() {
        let config = Config::default();
        let (_temp_dir, session) = create_test_session();
        let result = print_steps(&config, &session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Step 0"));
        assert!(output.contains("Test prompt"));
    }

    #[test]
    fn test_print_steps_with_patch() {
        let config = Config::default();
        let (_temp_dir, mut session) = create_test_session();
        let patch = Patch {
            comment: Some("Test comment".to_string()),
            ..Default::default()
        };
        session.set_last_patch(&patch);
        let result = print_steps(&config, &session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Step 0"));
        assert!(output.contains("Test prompt"));
        assert!(output.contains("comment:"));
        assert!(output.contains("Test comment"));
    }

    #[test]
    fn test_print_steps_with_error() {
        let config = Config::default();
        let (_temp_dir, mut session) = create_test_session();
        session.set_last_error(&TenxError::Internal("Test error".to_string()));
        let result = print_steps(&config, &session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Step 0"));
        assert!(output.contains("Test prompt"));
        assert!(output.contains("error:"));
        assert!(output.contains("Test error"));
    }

    #[test]
    fn test_render_step_editable() {
        let step = Step {
            prompt: Prompt::User("Test prompt\nwith multiple\nlines".to_string()),
            patch: None,
            err: None,
            usage: None,
        };
        let full_result = render_step_prompt(&step, 80, true);
        assert!(full_result.contains("Test prompt"));
        assert!(full_result.contains("with multiple"));
        assert!(full_result.contains("lines"));
    }
}
