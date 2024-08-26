use colored::*;
use libtenx::{dialect::DialectProvider, model::ModelProvider, Result, Session, TenxError};
use std::collections::HashSet;
use terminal_size::{terminal_size, Width};
use textwrap::{wrap, Options};

const DEFAULT_WIDTH: usize = 80;
const INDENT: &str = "  ";

/// Pretty prints the Session information.
pub fn session(session: &Session, full: bool) -> Result<String> {
    let width = terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(DEFAULT_WIDTH);
    let mut output = String::new();
    output.push_str(&print_session_info(session));
    output.push_str(&print_context(session));
    output.push_str(&print_editables(session)?);
    output.push_str(&print_steps(session, full, width)?);
    Ok(output)
}

fn print_session_info(session: &Session) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{} {}\n",
        "root:".blue().bold(),
        session.root.display()
    ));
    output.push_str(&format!(
        "{} {}\n",
        "dialect:".blue().bold(),
        session.dialect.name()
    ));
    output.push_str(&format!(
        "{} {}\n",
        "model:".blue().bold(),
        session
            .model
            .as_ref()
            .map_or_else(|| "None".to_string(), |m| m.name().to_string())
    ));
    output
}

fn print_context(session: &Session) -> String {
    let mut output = String::new();
    if !session.context().is_empty() {
        output.push_str(&format!("{}\n", "context:".blue().bold()));
        for context in session.context() {
            output.push_str(&format!("{}- {:?}: {}\n", INDENT, context.ty, context.name));
        }
    }
    output
}

fn print_editables(session: &Session) -> Result<String> {
    let mut output = String::new();
    let editables = session.editables()?;
    if !editables.is_empty() {
        output.push_str(&format!("{}\n", "edit:".blue().bold()));
        for path in editables {
            output.push_str(&format!(
                "{}- {}\n",
                INDENT,
                session.relpath(&path).display()
            ));
        }
    }
    Ok(output)
}

fn print_steps(session: &Session, full: bool, width: usize) -> Result<String> {
    let mut output = String::new();
    if session.steps().is_empty() {
        return Err(TenxError::Internal("Session has no steps".to_string()));
    }
    output.push_str(&format!("{}\n", "steps:".blue().bold()));
    for (i, step) in session.steps().iter().enumerate() {
        output.push_str(&format!("{}{}: ", INDENT, format!("{}", i).cyan().bold()));
        output.push_str(&render_step_editable(step, full, width));
        output.push('\n');
        if let Some(patch) = &step.patch {
            output.push_str(&print_patch(session, patch, full, width));
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
    }
    Ok(output)
}

fn render_step_editable(step: &libtenx::Step, full: bool, width: usize) -> String {
    let prompt = if full {
        &step.prompt.user_prompt
    } else {
        step.prompt.user_prompt.lines().next().unwrap_or("")
    };
    format!("\n{}", wrapped_block(prompt, width, INDENT.len() * 2))
}

fn print_patch(
    session: &Session,
    patch: &libtenx::patch::Patch,
    full: bool,
    width: usize,
) -> String {
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
    let modified_files: HashSet<_> = patch
        .changes
        .iter()
        .map(|change| match change {
            libtenx::patch::Change::Write(w) => &w.path,
            libtenx::patch::Change::Replace(r) => &r.path,
        })
        .collect();
    if !modified_files.is_empty() {
        output.push_str(&format!(
            "{}{}\n",
            INDENT.repeat(2),
            "modified:".blue().bold()
        ));
        for file in modified_files {
            output.push_str(&format!(
                "{}- {}\n",
                INDENT.repeat(3),
                session.relpath(file).display()
            ));
        }
    }
    output
}

/// Pretty prints a TenxError with full details.
pub fn full_error(error: &TenxError) -> String {
    match error {
        TenxError::Validation { name, user, model } => {
            format!(
                "Validation Error: {}\nUser Message: {}\nModel Message: {}",
                name, user, model
            )
        }
        TenxError::Patch { user, model } => {
            format!(
                "Patch Error\nUser Message: {}\nModel Message: {}",
                user, model
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
    use libtenx::{
        dialect::Tags, model::Claude, patch::Patch, prompt::PromptInput, Context, ContextData,
        ContextType, Step, TenxError,
    };
    use tempfile::TempDir;

    fn create_test_session() -> (TempDir, Session) {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();
        let mut session = Session::new(
            root_path.clone(),
            libtenx::dialect::Dialect::Tags(Tags::default()),
            libtenx::model::Model::Claude(Claude::default()),
        );
        session
            .add_prompt(PromptInput {
                user_prompt: "Test prompt".to_string(),
            })
            .unwrap();
        session.add_context(Context {
            ty: ContextType::File,
            name: "test_file.rs".to_string(),
            data: ContextData::String("Test content".to_string()),
        });
        (temp_dir, session)
    }

    #[test]
    fn test_print_steps_empty_session() {
        let (_temp_dir, session) = create_test_session();
        let result = print_steps(&session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("steps:"));
        assert!(output.contains("0:"));
        assert!(output.contains("Test prompt"));
    }

    #[test]
    fn test_print_steps_with_patch() {
        let (_temp_dir, mut session) = create_test_session();
        let mut patch = Patch::default();
        patch.comment = Some("Test comment".to_string());
        session.set_last_patch(&patch);
        let result = print_steps(&session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("steps:"));
        assert!(output.contains("0:"));
        assert!(output.contains("Test prompt"));
        assert!(output.contains("comment:"));
        assert!(output.contains("Test comment"));
    }

    #[test]
    fn test_print_steps_with_error() {
        let (_temp_dir, mut session) = create_test_session();
        session.set_last_error(&TenxError::Internal("Test error".to_string()));
        let result = print_steps(&session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("steps:"));
        assert!(output.contains("0:"));
        assert!(output.contains("Test prompt"));
        assert!(output.contains("error:"));
        assert!(output.contains("Test error"));
    }

    #[test]
    fn test_render_step_editable() {
        let step = Step {
            prompt: PromptInput {
                user_prompt: "Test prompt\nwith multiple\nlines".to_string(),
            },
            patch: None,
            err: None,
        };
        let result = render_step_editable(&step, false, 80);
        assert!(result.contains("Test prompt"));
        assert!(!result.contains("with multiple"));

        let full_result = render_step_editable(&step, true, 80);
        assert!(full_result.contains("Test prompt"));
        assert!(full_result.contains("with multiple"));
        assert!(full_result.contains("lines"));
    }
}
