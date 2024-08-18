use libtenx::{Result, Session};

use colored::*;

/// Pretty prints the Session information.
pub fn session(session: &Session) -> Result<String> {
    let mut output = String::new();
    output.push_str(&format!(
        "{} {}\n",
        "Rooot Directory:".blue().bold(),
        session.root.display()
    ));
    output.push_str(&format!(
        "{} {:?}\n",
        "Dialect:".blue().bold(),
        session.dialect
    ));
    output.push_str(&format!("{}\n", "Context:".blue().bold()));
    for context in &session.context {
        output.push_str(&format!("  - {:?}: {}\n", context.ty, context.name));
    }
    output.push_str(&format!("{}\n", "Edit Paths:".blue().bold()));
    for path in session.editables()? {
        output.push_str(&format!("  - {}\n", path.display()));
    }
    Ok(output)
}
