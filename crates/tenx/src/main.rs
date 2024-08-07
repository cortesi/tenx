use anyhow::{Context as AnyhowContext, Result};
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

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
        #[clap(short, long)]
        prompt: Option<String>,

        /// Show the generated context
        #[clap(long)]
        show_context: bool,

        /// Show the generated prompt
        #[clap(long)]
        show_prompt: bool,

        /// Show the discovered workspace
        #[clap(long)]
        show_workspace: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Info { path } => {
            // Handle 'info' command
            println!("Handling 'info' command with path: {:?}", path);
            Ok(())
        }
        Commands::Edit {
            files,
            attach,
            prompt,
            show_context,
            show_prompt,
            show_workspace,
        } => {
            // Create Context and Workspace using the new function
            let (context, workspace) = initialise(
                files.clone(),
                attach.clone(),
                prompt.clone().unwrap_or_default(),
            )
            .context("Failed to create Context and Workspace")?;

            if *show_context {
                println!("{}", "Context:".green().bold());
                println!("{:#?}", context);
            }

            if *show_workspace {
                println!("{}", "Workspace:".yellow().bold());
                println!("{:#?}", workspace);
            }

            let c = Claude::new();
            let rendered_prompt = c.render(&context, &workspace).await?;

            if *show_prompt {
                println!("{}", "Prompt:".blue().bold());
                println!("{:#?}", rendered_prompt);
            }

            Ok(())
        }
    }
}
