use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
    process::Command,
};

use anyhow::{Context as AnyhowContext, Result};
use clap::{Parser, Subcommand};
use colored::*;
use tempfile::NamedTempFile;

use libtenx::{self, initialise, Claude};

#[derive(Parser)]
#[clap(name = "tenx")]
#[clap(author = "Aldo Cortesi")]
#[clap(version = "0.1.0")]
#[clap(about = "AI-powered command-line assistant for Rust", long_about = None)]
struct Cli {
    /// Increase output verbosity
    #[clap(short, long, action = clap::ArgAction::Count, global = true, default_value = "1")]
    verbose: u8,

    /// Decrease output verbosity
    #[clap(short, long, global = true)]
    quiet: bool,

    /// Anthropic API key
    #[clap(long, env = "ANTHROPIC_API_KEY", hide_env_values = true, global = true)]
    anthropic_key: Option<String>,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get information about the current project
    Info {
        /// Sets the project path
        #[clap(short, long, value_parser)]
        path: Option<PathBuf>,
    },
    /// Edit files in the project
    Edit {
        /// Specifies files to edit
        #[clap(required = true, value_parser)]
        files: Vec<PathBuf>,

        /// Specifies files to attach (but not edit)
        #[clap(short, long, value_parser)]
        attach: Vec<PathBuf>,

        /// User prompt for the edit operation
        #[clap(long)]
        prompt: Option<String>,

        /// Path to a file containing the prompt
        #[clap(long)]
        prompt_file: Option<PathBuf>,

        /// Don't apply changes, just show what would be done
        #[clap(long)]
        dry_run: bool,
    },
}

/// Returns the user's preferred editor.
fn get_editor() -> String {
    env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
}

/// Creates a comment block listing editable and context files.
fn create_file_comment(files: &[PathBuf], attach: &[PathBuf]) -> String {
    let mut comment = String::from("\n# Files to edit:\n");
    for file in files {
        comment.push_str(&format!("# - {}\n", file.display()));
    }
    if !attach.is_empty() {
        comment.push_str("#\n# Context files (not editable):\n");
        for file in attach {
            comment.push_str(&format!("# - {}\n", file.display()));
        }
    }
    comment.push_str("#\n");
    comment
}

/// Removes comment lines from the user's prompt.
fn strip_comments(input: &str) -> String {
    input
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<&str>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Opens an editor for the user to input their prompt.
fn edit_prompt(files: &[PathBuf], attach: &[PathBuf]) -> Result<String> {
    let mut temp_file = NamedTempFile::new()?;
    let comment = create_file_comment(files, attach);
    temp_file.write_all(comment.as_bytes())?;
    temp_file.flush()?;

    let editor = get_editor();
    Command::new(editor)
        .arg(temp_file.path())
        .status()
        .context("Failed to open editor")?;

    let content = fs::read_to_string(temp_file.path()).context("Failed to read temporary file")?;
    Ok(strip_comments(&content))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Info { path } => {
            println!("Handling 'info' command with path: {:?}", path);
            Ok(())
        }
        Commands::Edit {
            files,
            attach,
            prompt,
            prompt_file,
            dry_run,
        } => {
            let verbosity = if cli.quiet { 0 } else { cli.verbose };

            let user_prompt = if let Some(p) = prompt {
                p.clone()
            } else if let Some(file_path) = prompt_file {
                fs::read_to_string(file_path).context("Failed to read prompt file")?
            } else {
                edit_prompt(files, attach)?
            };

            let mut context = initialise(files.clone(), attach.clone(), user_prompt)
                .context("Failed to create Context and Workspace")?;

            if verbosity >= 2 {
                println!("{}", "Context:".green().bold());
                println!("{:#?}", context);
            }

            let c = Claude::new(cli.anthropic_key.as_deref().unwrap_or(""))?;
            let mut request = c.render(&context).await?;
            if verbosity >= 2 {
                println!("{}", "Query:".blue().bold());
                println!("{:#?}", request);
            }
            print!("{} ", "Claude:".blue().bold());
            let response = c
                .stream_response(&request, |chunk| {
                    print!("{}", chunk);
                    io::stdout().flush()?;
                    Ok(())
                })
                .await?;
            request.merge_response(&response);

            let ops = libtenx::extract_operations(&request)?;

            if *dry_run {
                println!(
                    "\n{}",
                    "Dry run: Changes that would be applied:".yellow().bold()
                );
                println!("{:#?}", ops);
            } else {
                context.apply_all(&ops)?;
                if verbosity >= 1 {
                    println!("\n{}", "Applied changes:".green().bold());
                    println!("{:#?}", ops);
                }
            }

            Ok(())
        }
    }
}

