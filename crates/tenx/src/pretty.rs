use colored::*;
use libtenx::{dialect::DialectProvider, Result, Session, TenxError};
use std::collections::HashSet;

/// Pretty prints the Session information.
pub fn session(session: &Session, verbose: bool) -> Result<String> {
    let mut output = String::new();
    output.push_str(&print_session_info(session));
    output.push_str(&print_context(session));
    output.push_str(&print_editables(session)?);
    output.push_str(&print_steps(session, verbose)?);
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
            output.push_str(&format!("  - {:?}: {}\n", context.ty, context.name));
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
            output.push_str(&format!("  - {}\n", session.relpath(&path).display()));
        }
    }
    Ok(output)
}

fn print_steps(session: &Session, verbose: bool) -> Result<String> {
    let mut output = String::new();
    if !session.steps.is_empty() {
        output.push_str(&format!("{}\n", "steps:".blue().bold()));
        for (i, step) in session.steps.iter().enumerate() {
            output.push_str(&format!("  {}: ", format!("{}", i).cyan().bold()));
            if verbose {
                output.push_str(&step.prompt.user_prompt);
            } else {
                output.push_str(step.prompt.user_prompt.lines().next().unwrap_or(""));
            }
            output.push('\n');
            if let Some(patch) = &step.patch {
                output.push_str(&print_patch(session, patch, verbose));
            }
            if let Some(err) = &step.err {
                output.push_str(&format!("    {}\n", "error:".yellow().bold()));
                if verbose {
                    output.push_str(&verbose_error(err));
                } else {
                    output.push_str(&format!("      {}\n", err));
                }
            }
        }
    }
    Ok(output)
}

fn print_patch(session: &Session, patch: &libtenx::patch::Patch, verbose: bool) -> String {
    let mut output = String::new();
    if let Some(comment) = &patch.comment {
        output.push_str(&format!(
            "    {} {}\n",
            "comment:".blue().bold(),
            if verbose {
                comment
            } else {
                comment.lines().next().unwrap_or("")
            }
        ));
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
        output.push_str(&format!("    {}", "modified:\n".blue().bold()));
        for file in modified_files {
            output.push_str(&format!("      - {}\n", session.relpath(file).display()));
        }
    }
    output
}

/// Pretty prints a TenxError with full details.
pub fn verbose_error(error: &TenxError) -> String {
    match error {
        TenxError::Validation { name, user, model } => {
            format!(
                "Validation Error: {}\nUser Message: {}\nModel Message: {}\n",
                name, user, model
            )
        }
        TenxError::Patch { user, model } => {
            format!(
                "Patch Error\nUser Message: {}\nModel Message: {}\n",
                user, model
            )
        }
        _ => format!("{:?}\n", error),
    }
}

