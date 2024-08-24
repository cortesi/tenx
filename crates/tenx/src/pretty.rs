use colored::*;
use libtenx::{dialect::DialectProvider, Result, Session, TenxError};
use std::collections::HashSet;
use terminal_size::{terminal_size, Width};
use textwrap::{wrap, Options};

const DEFAULT_WIDTH: usize = 80;
const INDENT: &str = "  ";

/// Pretty prints the Session information.
pub fn session(session: &Session, verbose: bool) -> Result<String> {
    let width = terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(DEFAULT_WIDTH);
    let mut output = String::new();
    output.push_str(&print_session_info(session));
    output.push_str(&print_context(session));
    output.push_str(&print_editables(session)?);
    output.push_str(&print_steps(session, verbose, width)?);
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
    output
}

fn print_context(session: &Session) -> String {
    let mut output = String::new();
    if !session.context.is_empty() {
        output.push_str(&format!("{}\n", "context:".blue().bold()));
        for context in &session.context {
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

fn print_steps(session: &Session, verbose: bool, width: usize) -> Result<String> {
    let mut output = String::new();
    if !session.steps.is_empty() {
        output.push_str(&format!("{}\n", "steps:".blue().bold()));
        for (i, step) in session.steps.iter().enumerate() {
            output.push_str(&format!("{}{}: ", INDENT, format!("{}", i).cyan().bold()));
            let prompt = if verbose {
                step.prompt.user_prompt.clone()
            } else {
                step.prompt
                    .user_prompt
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string()
            };
            output.push_str(&wrap_text(&prompt, width, INDENT.len() + 3));
            output.push('\n');
            if let Some(patch) = &step.patch {
                output.push_str(&print_patch(session, patch, verbose, width));
            }
            if let Some(err) = &step.err {
                output.push_str(&format!(
                    "{}{}\n",
                    INDENT.repeat(2),
                    "error:".yellow().bold()
                ));
                if verbose {
                    output.push_str(&wrap_text(&verbose_error(err), width, INDENT.len() * 3));
                } else {
                    output.push_str(&wrap_text(&format!("{}", err), width, INDENT.len() * 3));
                }
                output.push('\n');
            }
        }
    }
    Ok(output)
}

fn print_patch(
    session: &Session,
    patch: &libtenx::patch::Patch,
    verbose: bool,
    width: usize,
) -> String {
    let mut output = String::new();
    if let Some(comment) = &patch.comment {
        output.push_str(&format!(
            "{}{} ",
            INDENT.repeat(2),
            "comment:".blue().bold()
        ));
        let comment_text = if verbose {
            comment.clone()
        } else {
            comment.lines().next().unwrap_or("").to_string()
        };
        output.push_str(&wrap_text(&comment_text, width, INDENT.len() * 2 + 9));
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
pub fn verbose_error(error: &TenxError) -> String {
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

fn wrap_text(text: &str, width: usize, initial_indent: usize) -> String {
    let ident = " ".repeat(initial_indent);
    let options = Options::new(width)
        .initial_indent(&ident)
        .subsequent_indent(&ident);
    wrap(text, &options).join("\n")
}

