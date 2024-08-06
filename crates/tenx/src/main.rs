use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

use libtenx::{Claude, Query};

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

        /// Show the generated query
        #[clap(long)]
        show_query: bool,

        /// Show the generated prompt
        #[clap(long)]
        show_prompt: bool,
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
            show_query,
            show_prompt,
        } => {
            // Construct a Query from the provided file paths
            let query = Query::new(
                files.clone(),
                attach.clone(),
                prompt.clone().unwrap_or_default(),
            )
            .context("Failed to create Query")?;

            if *show_query {
                println!("{}", "Query:".green().bold());
                println!("{:#?}", query);
            }

            let c = Claude::new();
            let rendered_prompt = c.render(&query).await?;

            if *show_prompt {
                println!("{}", "Prompt:".blue().bold());
                println!("{}", rendered_prompt);
            }

            Ok(())
        }
    }
}
