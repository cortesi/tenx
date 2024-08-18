use colored::*;
use libtenx::{dialect::DialectProvider, Result, Session};
use std::collections::HashSet;

/// Pretty prints the Session information.
pub fn session(session: &Session) -> Result<String> {
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
    if !session.context.is_empty() {
        output.push_str(&format!("{}\n", "context:".blue().bold()));
        for context in &session.context {
            output.push_str(&format!("  - {:?}: {}\n", context.ty, context.name));
        }
    }
    let editables = session.editables()?;
    if !editables.is_empty() {
        output.push_str(&format!("{}\n", "edit:".blue().bold()));
        for path in editables {
            output.push_str(&format!("  - {}\n", session.relpath(&path).display()));
        }
    }
    if !session.steps.is_empty() {
        output.push_str(&format!("{}\n", "steps:".blue().bold()));
        for (i, step) in session.steps.iter().enumerate() {
            output.push_str(&format!("  {}: ", i));
            output.push_str(step.prompt.user_prompt.lines().next().unwrap_or(""));
            output.push('\n');
            if let Some(patch) = &step.patch {
                if let Some(comment) = &patch.comment {
                    output.push_str(&format!(
                        "    comment: {}\n",
                        comment.lines().next().unwrap_or("")
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
                    output.push_str("    modified:\n");
                    for file in modified_files {
                        output.push_str(&format!("      - {}\n", session.relpath(file).display()));
                    }
                }
            }
        }
    }
    Ok(output)
}

