//! Pretty-printing functionality for various Tenx objects.
use crate::{
    config::Config,
    context,
    context::ContextProvider,
    model, patch,
    session::{Operation, Session, Step},
    Result, TenxError,
};
use colored::*;
use textwrap::{indent, wrap, Options};

const INDENT: &str = "  ";

fn get_term_width() -> usize {
    termsize::get()
        .map(|size| size.cols as usize)
        .unwrap_or(120)
}

fn format_usage(usage: &model::Usage) -> String {
    let values = usage.values();
    let mut keys: Vec<_> = values.keys().collect();
    keys.sort();
    keys.iter()
        .map(|k| format!("{}: {}", k.blue().bold(), values.get(*k).unwrap()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Converts a path to use ~ for paths under the user's home directory
fn display_path_with_home(path: &std::path::Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = std::path::Path::new(&home);
        if let Ok(rel_path) = path.strip_prefix(home_path) {
            format!("~/{}", rel_path.display())
        } else {
            path.display().to_string()
        }
    } else {
        path.display().to_string()
    }
}

fn print_session_info(config: &Config, _: &Session) -> String {
    let mut output = String::new();
    let display_path = display_path_with_home(&config.project_root());
    output.push_str(&format!("{} {}\n", "root:".blue().bold(), display_path));
    output
}

fn print_context_specs(session: &Session) -> String {
    let mut output = String::new();
    if !session.contexts().is_empty() {
        output.push_str(&format!("{}\n", "context:".blue().bold()));
        for context in session.contexts() {
            output.push_str(&format!("{}- {}\n", INDENT, context.human()));
        }
    }
    output
}

fn print_editables(_config: &Config, session: &Session) -> Result<String> {
    let mut output = String::new();
    let editables = session.editables();
    if !editables.is_empty() {
        output.push_str(&format!("{}\n", "edit:".blue().bold()));
        for path in editables {
            output.push_str(&format!("{}- {}\n", INDENT, path.display()));
        }
    }
    Ok(output)
}

fn print_operations(config: &Config, operations: &[Operation]) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{}{}\n",
        INDENT.repeat(2),
        "operations:".blue().bold()
    ));
    for op in operations {
        match op {
            Operation::Edit(path) => {
                output.push_str(&format!(
                    "{}- edit: {}\n",
                    INDENT.repeat(3),
                    config.relpath(path).display()
                ));
            }
        }
    }
    output
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
        if let Some(response) = &step.model_response {
            if let Some(comment) = &response.comment {
                output.push_str(&format!(
                    "{}{}\n",
                    INDENT.repeat(2),
                    "model comment:".blue().bold()
                ));
                let comment_text = if full {
                    comment.clone()
                } else {
                    comment.lines().next().unwrap_or("").to_string()
                };
                output.push_str(&wrapped_block(&comment_text, width, INDENT.len() * 3));
                output.push('\n');
            }
            if let Some(text) = &response.response_text {
                if full {
                    output.push_str(&format!(
                        "{}{}\n",
                        INDENT.repeat(2),
                        "raw model response:".blue().bold()
                    ));
                    output.push_str(&wrapped_block(&text.clone(), width, INDENT.len() * 3));
                    output.push('\n');
                }
            }

            if !response.operations.is_empty() {
                output.push_str(&print_operations(config, &response.operations));
            }
            if let Some(patch) = &response.patch {
                output.push_str(&print_patch(config, patch, full, width));
            }
            if let Some(usage) = &response.usage {
                output.push_str(&format!("{}{}\n", INDENT.repeat(2), "usage:".blue().bold()));
                for line in format_usage(usage).lines() {
                    output.push_str(&format!("{}{}\n", INDENT.repeat(3), line));
                }
            }
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

fn render_step_prompt(step: &Step, width: usize, _full: bool) -> String {
    let prompt_header = format!("{}{}\n", INDENT.repeat(2), "prompt:".blue().bold());
    let text = &step.prompt;
    format!(
        "{}{}",
        prompt_header,
        wrapped_block(text, width, INDENT.len() * 3)
    )
}

fn print_patch(config: &Config, patch: &patch::Patch, full: bool, width: usize) -> String {
    use std::collections::HashMap;

    let mut output = String::new();
    output.push_str(&format!(
        "{}{}\n",
        INDENT.repeat(2),
        "modified:".blue().bold()
    ));

    // Group changes by file path
    let mut changes_by_file: HashMap<&std::path::Path, Vec<&patch::Change>> = HashMap::new();
    for change in &patch.changes {
        let path = match change {
            patch::Change::Write(w) => &w.path,
            patch::Change::Replace(r) => &r.path,
            patch::Change::View(p) => p,
        };
        changes_by_file.entry(path).or_default().push(change);
    }

    for (path, changes) in changes_by_file {
        let file_path = config.relpath(path).display().to_string().green().bold();
        output.push_str(&format!("{}- {}\n", INDENT.repeat(3), file_path));

        // Count changes by type
        let mut write_count = 0;
        let mut replace_count = 0;
        for change in &changes {
            match change {
                patch::Change::Write(_) => write_count += 1,
                patch::Change::Replace(_) => replace_count += 1,
                patch::Change::View(_) => (),
            }
        }

        let mut types = Vec::new();
        if write_count > 0 {
            types.push(format!("write ({})", write_count));
        }
        if replace_count > 0 {
            types.push(format!("replace ({})", replace_count));
        }

        output.push_str(&format!("{}{}\n", INDENT.repeat(4), types.join(", ")));

        if full {
            for change in &changes {
                match *change {
                    patch::Change::Write(w) => {
                        output.push_str(&wrapped_block(&w.content, width, INDENT.len() * 5));
                        output.push('\n');
                    }
                    patch::Change::Replace(r) => {
                        output.push_str(&format!(
                            "{}{}\n",
                            INDENT.repeat(5),
                            "old:".yellow().bold()
                        ));
                        output.push_str(&wrapped_block(&r.old, width, INDENT.len() * 6));
                        output.push_str(&format!(
                            "\n{}{}\n",
                            INDENT.repeat(5),
                            "new:".green().bold()
                        ));
                        output.push_str(&wrapped_block(&r.new, width, INDENT.len() * 6));
                        output.push('\n');
                    }
                    patch::Change::View(_) => (),
                }
            }
        }
    }

    output
}

/// Pretty prints a TenxError with full details.
fn full_error(error: &TenxError) -> String {
    match error {
        TenxError::Check { name, user, model } => {
            format!(
                "{}: {}\n{}: {}\n{}: {}",
                "Check Error".red().bold(),
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

/// Pretty prints a context item with optional full detail
fn print_context_item(item: &context::ContextItem) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "{}{}: {}\n",
        INDENT.repeat(2),
        item.ty.blue().bold(),
        item.source
    ));

    output.push_str(&wrapped_block(
        &item.body,
        get_term_width(),
        INDENT.len() * 3,
    ));
    output.push('\n');

    output
}

/// Pretty prints the Session information.
pub fn print_session(config: &Config, session: &Session, full: bool) -> Result<String> {
    let width = get_term_width();
    let mut output = String::new();
    output.push_str(&format!("{}\n", "session:".blue().bold()));
    output.push_str(&indent(
        &format!(
            "{}{}{}{}",
            print_session_info(config, session),
            print_context_specs(session),
            print_editables(config, session)?,
            print_steps(config, session, full, width - INDENT.len())?
        ),
        INDENT,
    ));
    Ok(output)
}

/// Pretty prints project information
pub fn print_project(config: &Config) -> String {
    let mut output = String::new();
    let display_path = display_path_with_home(&config.project_root());
    output.push_str(&format!(
        "{} {}\n",
        "project root:".blue().bold(),
        display_path
    ));
    if !config.project.include.is_empty() {
        output.push_str(&format!(
            "{} {}\n",
            "globs:".blue().bold(),
            config
                .project
                .include
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    output
}

/// Pretty prints all contexts in a session
pub fn print_contexts(config: &Config, session: &Session) -> Result<String> {
    let mut output = String::new();
    for context in session.contexts() {
        let items = context.context_items(config, &Session::new(config)?)?;
        if let Some(item) = items.into_iter().next() {
            output.push_str(&print_context_item(&item));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::Context,
        patch::Patch,
        session::{ModelResponse, Step},
        strategy, testutils, TenxError,
    };

    fn create_test_project() -> testutils::TestProject {
        let mut p = testutils::test_project();
        p.session
            .add_action(
                &p.config,
                strategy::Strategy::Code(strategy::Code::new("test".into())),
            )
            .unwrap();
        p.session
            .add_step("test_model".into(), "Test prompt".to_string())
            .unwrap();
        p.write("test_file.rs", "Test content");
        p.session
            .add_context(Context::new_path(&p.config, "test_file.rs").unwrap());
        p
    }

    #[test]
    fn test_print_steps_empty_session() {
        let config = Config::default();
        let p = create_test_project();
        let result = print_steps(&config, &p.session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Step 0"));
        assert!(output.contains("Test prompt"));
    }

    #[test]
    fn test_print_steps_with_patch() {
        let config = Config::default();
        let mut p = create_test_project();

        if let Some(step) = p.session.last_step_mut() {
            step.model_response = Some(ModelResponse {
                patch: Some(Patch {
                    ..Default::default()
                }),
                operations: vec![],
                usage: None,
                comment: Some("Test comment".to_string()),
                response_text: Some("Test comment".to_string()),
            });
        }
        let result = print_steps(&config, &p.session, false, 80);
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
        let mut p = create_test_project();
        if let Some(step) = p.session.last_step_mut() {
            step.err = Some(TenxError::Internal("Test error".to_string()));
        }
        let result = print_steps(&config, &p.session, false, 80);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Step 0"));
        assert!(output.contains("Test prompt"));
        assert!(output.contains("error:"));
        assert!(output.contains("Test error"));
    }

    #[test]
    fn test_render_step_editable() {
        let step = Step::new(
            "test_model".into(),
            "Test prompt\nwith multiple\nlines".to_string(),
        );
        let full_result = render_step_prompt(&step, 80, true);
        assert!(full_result.contains("Test prompt"));
        assert!(full_result.contains("with multiple"));
        assert!(full_result.contains("lines"));
    }
}
