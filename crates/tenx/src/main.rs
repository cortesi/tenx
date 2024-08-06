use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use libtenx::Query;

#[derive(Parser)]
#[clap(name = "tenx")]
#[clap(author = "Your Name")]
#[clap(version = "0.1.0")]
#[clap(about = "AI-powered command-line assistant", long_about = None)]
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
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Info { path } => {
            // Handle 'info' command
            println!("Handling 'info' command with path: {:?}", path);
            Ok(())
        }
        Commands::Edit { files } => {
            // Handle 'edit' command
            println!("Handling 'edit' command with files: {:?}", files);

            // Construct a Query from the provided file paths
            let query = Query::from_edits(files.clone())
                .context("Failed to create Query from edit paths")?;

            println!("Created Query: {:?}", query);
            Ok(())
        }
    }
}
