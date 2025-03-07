//! Pretty-printing functionality for various Tenx objects.
use crate::{
    config::Config,
    context,
    context::ContextProvider,
    error::{Result, TenxError},
    model, patch,
    session::{Operation, Session, Step},
    strategy::ActionStrategy,
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
    if !session.contexts.is_empty() {
        output.push_str(&format!("{}\n", "context:".blue().bold()));
        for context in &session.contexts {
            output.push_str(&format!("{}- {}\n", INDENT, context.human()));
        }
    }
    output
}

fn print_operations(_config: &Config, _operations: &[Operation]) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{}{}\n",
        INDENT.repeat(2),
        "operations:".blue().bold()
    ));
    output
}

/// Renders a step offset in the format "action" or "action:step"
pub fn render_step_offset(action_idx: usize, step_idx: Option<usize>) -> String {
    match step_idx {
        Some(step) => format!("{}:{}", action_idx, step),
        None => format!("{}", action_idx),
    }
}

fn print_steps(config: &Config, session: &Session, full: bool, width: usize) -> Result<String> {
    if session.steps().is_empty() {
        return Ok(String::new());
    }

    let mut output = String::new();

    // Group steps by action
    for (action_idx, action) in session.actions.iter().enumerate() {
        // Print action header with strategy name
        output.push_str(&format!("\n{}\n", "=".repeat(width)));
        output.push_str(&format!(
            "{} {}\n",
            format!("Action {}", action_idx).cyan().bold(),
            format!("({})", action.strategy.name()).yellow()
        ));
        output.push_str(&format!("{}\n", "=".repeat(width)));

        // Print each step in this action
        let action_steps = action.steps();
        for (local_step_idx, step) in action_steps.iter().enumerate() {
            // Print step header
            output.push_str(&format!("\n{}\n", "-".repeat(width - 10)));
            output.push_str(&format!(
                "{}\n",
                format!(
                    "Step {}",
                    render_step_offset(action_idx, Some(local_step_idx))
                )
                .blue()
                .bold()
            ));
            output.push_str(&format!("{}\n", "-".repeat(width - 10)));

            // Print step content
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
                if let Some(text) = &response.raw_response {
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
    }

    Ok(output)
}

fn render_step_prompt(step: &Step, width: usize, _full: bool) -> String {
    let prompt_header = format!("{}{}\n", INDENT.repeat(2), "prompt:".blue().bold());
    let text = &step.raw_prompt;
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
            "{}{}{}",
            print_session_info(config, session),
            print_context_specs(session),
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
    for context in &session.contexts {
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
        session::{Action, ModelResponse, Step},
        strategy, testutils,
    };

    fn create_test_project() -> Result<testutils::TestProject> {
        let mut p = testutils::test_project();
        p.session.add_action(Action::new(
            &p.config,
            strategy::Strategy::Code(strategy::Code::new()),
        )?)?;
        p.session
            .last_action_mut()?
            .add_step(Step::new("test_model".into(), "Test prompt".to_string()))
            .unwrap();
        p.write("test_file.rs", "Test content");
        p.session
            .add_context(Context::new_path(&p.config, "test_file.rs").unwrap());
        Ok(p)
    }

    #[test]
    fn test_print_steps_empty_session() -> Result<()> {
        let config = Config::default();
        let p = create_test_project()?;
        let output = print_steps(&config, &p.session, false, 80)?;
        assert!(output.contains("Step 0:0"));
        assert!(output.contains("Test prompt"));
        Ok(())
    }

    #[test]
    fn test_print_steps_with_patch() -> Result<()> {
        let config = Config::default();
        let mut p = create_test_project()?;
        if let Some(step) = p.session.last_step_mut() {
            step.model_response = Some(ModelResponse {
                patch: Some(Patch {
                    ..Default::default()
                }),
                operations: vec![],
                usage: None,
                comment: Some("Test comment".to_string()),
                raw_response: Some("Test comment".to_string()),
            });
        }
        let output = print_steps(&config, &p.session, false, 80)?;
        assert!(output.contains("Step 0:0"));
        assert!(output.contains("Test prompt"));
        assert!(output.contains("comment:"));
        assert!(output.contains("Test comment"));
        Ok(())
    }

    #[test]
    fn test_print_steps_with_error() -> Result<()> {
        let config = Config::default();
        let mut p = create_test_project()?;
        if let Some(step) = p.session.last_step_mut() {
            step.err = Some(TenxError::Internal("Test error".to_string()));
        }
        let output = print_steps(&config, &p.session, false, 80)?;
        assert!(output.contains("Step 0:0"));
        assert!(output.contains("Test prompt"));
        assert!(output.contains("error:"));
        assert!(output.contains("Test error"));
        Ok(())
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

    #[test]
    fn test_render_step_offset() {
        assert_eq!(render_step_offset(0, None), "0");
        assert_eq!(render_step_offset(1, Some(2)), "1:2");
        assert_eq!(render_step_offset(5, Some(0)), "5:0");
    }
}
