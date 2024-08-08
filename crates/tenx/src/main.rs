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

        /// Show the generated context
        #[clap(long)]
        show_context: bool,

        /// Show the query that will be sent to the model
        #[clap(long)]
        show_query: bool,

        /// Anthropic API key
        #[clap(long, env = "ANTHROPIC_API_KEY", hide_env_values = true)]
        anthropic_key: Option<String>,
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
            show_context,
            show_query,
            anthropic_key,
        } => {
            let user_prompt = if let Some(p) = prompt {
                p.clone()
            } else if let Some(file_path) = prompt_file {
                fs::read_to_string(file_path).context("Failed to read prompt file")?
            } else {
                edit_prompt()?
            };

            let mut context = initialise(files.clone(), attach.clone(), user_prompt)
                .context("Failed to create Context and Workspace")?;

            if *show_context {
                println!("{}", "Context:".green().bold());
                println!("{:#?}", context);
            }

            let c = Claude::new(anthropic_key.as_deref().unwrap_or(""))?;
            let mut request = c.render(&context).await?;
            if *show_query {
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
            context.apply_all(&ops)?;

            println!("\n{:#?}", ops);

            Ok(())
        }
    }
}
