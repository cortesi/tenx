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

        /// Increase output verbosity
        #[clap(short, long, action = clap::ArgAction::Count, default_value = "1")]
        verbose: u8,

        /// Decrease output verbosity
        #[clap(short, long)]
        quiet: bool,

        /// Anthropic API key
        #[clap(long, env = "ANTHROPIC_API_KEY", hide_env_values = true)]
        anthropic_key: Option<String>,

        /// Don't apply changes, just show what would be done
        #[clap(long)]
        dry_run: bool,
    },
}

fn get_editor() -> String {
    env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
}

fn edit_prompt() -> Result<String> {
    let temp_file = NamedTempFile::new()?;
    let editor = get_editor();
    Command::new(editor)
        .arg(temp_file.path())
        .status()
        .context("Failed to open editor")?;
    fs::read_to_string(temp_file.path()).context("Failed to read temporary file")
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
            verbose,
            quiet,
            anthropic_key,
            dry_run,
        } => {
            let verbosity = if *quiet { 0 } else { *verbose };

            let user_prompt = if let Some(p) = prompt {
                p.clone()
            } else if let Some(file_path) = prompt_file {
                fs::read_to_string(file_path).context("Failed to read prompt file")?
            } else {
                edit_prompt()?
            };

            let mut context = initialise(files.clone(), attach.clone(), user_prompt)
                .context("Failed to create Context and Workspace")?;

            if verbosity >= 2 {
                println!("{}", "Context:".green().bold());
                println!("{:#?}", context);
            }

            let c = Claude::new(anthropic_key.as_deref().unwrap_or(""))?;
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

