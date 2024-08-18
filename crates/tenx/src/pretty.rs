use libtenx::{dialect::DialectProvider, Result, Session};

use colored::*;

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
    Ok(output)
}
