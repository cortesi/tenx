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
    self, dialect::Dialect, model::Claude, model::Model, prompt::PromptInput, Config, Session, Tenx,
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

    /// Session storage directory (~/.config/tenx/state by default)
    #[clap(long, global = true)]
    session_store: Option<PathBuf>,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform an AI-assisted edit
    Edit {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Option<Vec<PathBuf>>,

        /// User prompt for the edit operation
        #[clap(long)]
        prompt: Option<String>,

        /// Path to a file containing the prompt
        #[clap(long)]
        prompt_file: Option<PathBuf>,
    },
    /// Create a new session
    New {
        /// Specifies files to add as context
        #[clap(value_parser)]
        files: Vec<PathBuf>,

        /// Add ruskel documentation
        #[clap(long)]
        ruskel: Vec<String>,
    },
    /// Add context to an existing session
    AddCtx {
        /// Specifies files to add as context
        #[clap(value_parser)]
        files: Vec<PathBuf>,

        /// Add ruskel documentation
        #[clap(long)]
        ruskel: Vec<String>,
    },
    /// Add editable files to an existing session
    AddEdit {
        /// Specifies files to add as editable
        #[clap(value_parser)]
        files: Vec<PathBuf>,
    },
    /// Retry the last prompt
    Retry,
    /// Show the current session
    Show,
}

/// Creates a Config from CLI arguments
fn load_config(cli: &Cli) -> Result<Config> {
    let mut config =
        Config::default().with_anthropic_key(cli.anthropic_key.clone().unwrap_or_default());
    if let Some(session_store_dir) = cli.session_store.clone() {
        config = config.with_session_store_dir(session_store_dir);
    }
    Ok(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let verbosity = if cli.quiet { 0 } else { cli.verbose };
    let subscriber = create_subscriber(verbosity);
    subscriber.init();

    match &cli.command {
        Commands::AddCtx { files, ruskel } => {
            let config = load_config(&cli)?;
            let tx = Tenx::new(config);
            let mut session = tx.load_session::<PathBuf>(None)?;

            for file in files {
                session.add_ctx_path(file)?;
            }

            for ruskel_doc in ruskel {
                session.add_ctx_ruskel(ruskel_doc.clone())?;
            }

            tx.save_session(session)?;
            info!("Context added to session successfully");
            Ok(())
        }
        Commands::Retry => {
            let config = load_config(&cli)?;
            let tx = Tenx::new(config);

            let (sender, mut receiver) = mpsc::channel(100);
            let print_task = tokio::spawn(async move {
                while let Some(chunk) = receiver.recv().await {
                    print!("{}", chunk);
                }
            });

            tx.retry::<PathBuf>(None, Some(sender)).await?;

            print_task.await?;
            info!("\n\n{}", "Changes applied successfully".green().bold());
            Ok(())
        }
        Commands::New { files, ruskel } => {
            let config = load_config(&cli)?;
            let tx = Tenx::new(config);
            let mut session = Session::new(
                None,
                Dialect::Tags(libtenx::dialect::Tags::default()),
                Model::Claude(Claude::default()),
            );

            for file in files {
                session.add_ctx_path(file)?;
            }

            for ruskel_doc in ruskel {
                session.add_ctx_ruskel(ruskel_doc.clone())?;
            }

            tx.save_session(session)?;
            info!("New session created successfully");
            Ok(())
        }
        Commands::Edit {
            files,
            prompt,
            prompt_file,
        } => {
            let config = load_config(&cli)?;
            let tx = Tenx::new(config);

            let (sender, mut receiver) = mpsc::channel(100);
            let print_task = tokio::spawn(async move {
                while let Some(chunk) = receiver.recv().await {
                    print!("{}", chunk);
                }
            });

            let mut session = tx.load_session::<PathBuf>(None)?;
            let user_prompt = if let Some(p) = prompt {
                PromptInput {
                    user_prompt: p.clone(),
                }
            } else if let Some(file_path) = prompt_file {
                let prompt_content =
                    fs::read_to_string(file_path).context("Failed to read prompt file")?;
                PromptInput {
                    user_prompt: prompt_content,
                }
            } else {
                let f = files.clone().unwrap_or_default();
                match edit::edit_prompt(&f)? {
                    Some(p) => p,
                    None => return Ok(()),
                }
            };
            session.add_prompt(user_prompt);
            for f in files.clone().unwrap_or_default() {
                session.add_editable(f)?;
            }

            tx.resume(&mut session, Some(sender)).await?;

            print_task.await?;
            info!("\n\n{}", "Changes applied successfully".green().bold());
            Ok(())
        }
        Commands::AddEdit { files } => {
            let config = load_config(&cli)?;
            let tx = Tenx::new(config);
            let mut session = tx.load_session::<PathBuf>(None)?;

            for file in files {
                session.add_editable(file)?;
            }

            tx.save_session(session)?;
            info!("Editable files added to session successfully");
            Ok(())
        }
        Commands::Show => {
            let config = load_config(&cli)?;
            let tx = Tenx::new(config);
            let session = tx.load_session::<PathBuf>(None)?;
            println!("{}", session.pretty_print());
            Ok(())
        }
    }
}

