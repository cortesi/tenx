use std::{fs, io, path::PathBuf};

use anyhow::{Context as AnyhowContext, Result};
use clap::{Parser, Subcommand};
use colored::*;
use tokio::sync::mpsc;
use tracing::{info, Subscriber};
use tracing_subscriber::fmt::format::{FmtSpan, Writer};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use libtenx::{
    self, dialect::Dialects, model::Claude, model::Models, Contents, DocType, Docs, Prompt, Tenx,
};

mod edit;

struct NoTime;

impl FormatTime for NoTime {
    fn format_time(&self, _: &mut Writer<'_>) -> std::fmt::Result {
        Ok(())
    }
}

/// Creates a subscriber that writes to stdout without timestamps.
fn create_subscriber(verbosity: u8) -> impl Subscriber {
    let filter = match verbosity {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    };

    fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_timer(NoTime)
        .with_writer(io::stdout)
        .with_span_events(FmtSpan::NONE)
        .finish()
}

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
    /// Edit files interactively
    Edit {
        /// Specifies files to edit
        #[clap(required = true, value_parser)]
        files: Vec<PathBuf>,

        /// Specifies files to attach (but not edit)
        #[clap(short, long, value_parser)]
        attach: Vec<PathBuf>,

        /// Add documentation file
        #[clap(long, value_parser)]
        docs: Vec<PathBuf>,

        /// Add ruskel documentation
        #[clap(long)]
        ruskel: Vec<String>,
    },
    /// Non-interactive editing of files
    Oneshot {
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

        /// Add documentation file
        #[clap(long, value_parser)]
        docs: Vec<PathBuf>,

        /// Add ruskel documentation
        #[clap(long)]
        ruskel: Vec<String>,
    },
}

/// Creates a vector of Docs from the provided paths and ruskel strings
async fn create_docs(docs: &Vec<PathBuf>, ruskel: &[String]) -> Result<Vec<Docs>> {
    let mut result = Vec::new();
    for path in docs {
        result.push(Docs {
            ty: DocType::Text,
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            contents: Contents::Path(path.clone()),
        });
    }
    for name in ruskel.iter() {
        result.push(Docs {
            ty: DocType::Ruskel,
            name: name.to_string(),
            contents: Contents::Unresolved(name.to_string()),
        });
    }
    Ok(result)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let verbosity = if cli.quiet { 0 } else { cli.verbose };
    let subscriber = create_subscriber(verbosity);
    subscriber.init();

    match &cli.command {
        Commands::Oneshot {
            files,
            attach,
            prompt,
            prompt_file,
            docs,
            ruskel,
        } => {
            let mut tx = Tenx::new(
                std::env::current_dir()?,
                Dialects::Tags(libtenx::dialect::Tags::default()),
                Models::Claude(Claude::default()),
            )
            .with_anthropic_key(cli.anthropic_key.clone().unwrap_or_default());

            let user_prompt = if let Some(p) = prompt {
                Prompt {
                    attach_paths: attach.clone(),
                    edit_paths: files.clone(),
                    user_prompt: p.clone(),
                    docs: create_docs(docs, ruskel).await?,
                }
            } else if let Some(file_path) = prompt_file {
                let prompt_content =
                    fs::read_to_string(file_path).context("Failed to read prompt file")?;
                Prompt {
                    attach_paths: attach.clone(),
                    edit_paths: files.clone(),
                    user_prompt: prompt_content,
                    docs: create_docs(docs, ruskel).await?,
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Either --prompt or --prompt-file must be provided"
                ));
            };

            tx.prompt(user_prompt, None).await?;

            info!("\n\n{}", "Changes applied successfully".green().bold());
            Ok(())
        }
        Commands::Edit {
            files,
            attach,
            docs,
            ruskel,
        } => {
            let mut tx = Tenx::new(
                std::env::current_dir()?,
                Dialects::Tags(libtenx::dialect::Tags::default()),
                Models::Claude(Claude::default()),
            )
            .with_anthropic_key(cli.anthropic_key.clone().unwrap_or_default());

            let (sender, mut receiver) = mpsc::channel(100);
            let print_task = tokio::spawn(async move {
                while let Some(chunk) = receiver.recv().await {
                    print!("{}", chunk);
                }
            });
            let mut user_prompt = edit::edit_prompt(files, attach)?;
            user_prompt.docs = create_docs(docs, ruskel).await?;

            tx.prompt(user_prompt, Some(sender)).await?;

            print_task.await?;
            info!("\n\n{}", "Changes applied successfully".green().bold());
            Ok(())
        }
    }
}
