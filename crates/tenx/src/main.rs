use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context as AnyhowContext, Result};
use clap::{CommandFactory, Parser, Subcommand};
use colored::*;
use serde_json::to_string_pretty;
use tokio::sync::mpsc;
use tracing::{info, Subscriber};
use tracing_subscriber::fmt::format::{FmtSpan, Writer};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use libtenx::{self, config, model::ModelProvider, prompt::Prompt, Event, Session, Tenx};

mod edit;
mod pretty;

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
#[clap(about = "AI-powered coding assistant", long_about = None)]
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

    /// Number of times to retry a prompt before failing
    #[clap(long, global = true)]
    retry_limit: Option<usize>,

    /// Skip preflight checks
    #[clap(long, global = true)]
    no_preflight: bool,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the current configuration
    Conf,
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
    /// Start a new session, edit the prompt, and run it
    Oneshot {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Vec<PathBuf>,

        /// Add ruskel documentation as context
        #[clap(long)]
        ruskel: Vec<String>,

        /// Add files as context
        #[clap(long)]
        ctx: Vec<PathBuf>,
    },
    /// Reset the session to a specific step
    Reset {
        /// The step offset to reset to
        step_offset: usize,
    },
    /// Retry a prompt
    Retry {
        /// The step offset to retry from
        step_offset: Option<usize>,
        /// Edit the prompt before retrying
        #[clap(long)]
        edit: bool,
        /// Add files as context
        #[clap(long)]
        ctx: Vec<PathBuf>,
        /// Add ruskel documentation as context
        #[clap(long)]
        ruskel: Vec<String>,
    },
    /// Show the current session
    Show {
        /// Print the entire session object verbosely
        #[clap(long)]
        raw: bool,

        /// Output the rendered session
        #[clap(long)]
        render: bool,

        /// Show full details
        #[clap(long)]
        full: bool,
    },
}

/// Creates a Config from CLI arguments
fn load_config(cli: &Cli) -> Result<config::Config> {
    let mut config = config::Config::default()
        .with_anthropic_key(cli.anthropic_key.clone().unwrap_or_default())
        .with_model(config::ConfigModel::Claude);
    if let Some(session_store_dir) = cli.session_store.clone() {
        config = config.with_session_store_dir(session_store_dir);
    }
    if let Some(retry_limit) = cli.retry_limit {
        config = config.with_retry_limit(retry_limit);
    }
    config = config.with_no_preflight(cli.no_preflight);
    Ok(config)
}

/// Prints events from the event channel
async fn print_events(mut receiver: mpsc::Receiver<Event>) {
    while let Some(event) = receiver.recv().await {
        match event {
            Event::Snippet(chunk) => {
                print!("{}", chunk);
                io::stdout().flush().unwrap();
            }
            Event::PreflightStart => println!("{}", "Starting preflight checks...".blue()),
            Event::PreflightEnd => println!("{}", "Preflight checks completed.".blue()),
            Event::PreflightOk(name) => println!(
                "\t{} {}",
                format!("'{}' passed.", name).green(),
                "✓".green()
            ),
            Event::FormattingStart => println!("{}", "Starting formatting...".blue()),
            Event::FormattingEnd => println!("{}", "Formatting completed.".blue()),
            Event::FormattingOk(name) => println!(
                "\t{} {}",
                format!("'{}' completed.", name).green(),
                "✓".green()
            ),
            Event::ValidationStart => println!("{}", "Starting post-patch validation...".blue()),
            Event::ValidationEnd => println!("{}", "Post-patch validation completed.".blue()),
            Event::ValidateOk(name) => println!(
                "\t{} {}",
                format!("'{}' passed.", name).green(),
                "✓".green()
            ),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let verbosity = if cli.quiet { 0 } else { cli.verbose };
    let subscriber = create_subscriber(verbosity);
    subscriber.init();

    match &cli.command {
        Some(cmd) => match cmd {
            Commands::Conf => {
                let config = load_config(&cli)?;
                println!("{}", to_string_pretty(&config)?);
                Ok(())
            }
            Commands::Oneshot { files, ruskel, ctx } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
                let mut session = Session::from_cwd()?;

                for file in ctx {
                    session.add_ctx_path(file)?;
                }

                for ruskel_doc in ruskel {
                    session.add_ctx_ruskel(ruskel_doc.clone())?;
                }

                for file in files {
                    session.add_editable(file)?;
                }

                session.add_prompt(Prompt::User(String::new()))?;
                match edit::edit_prompt(&session)? {
                    Some(p) => session.set_last_prompt(p)?,
                    None => return Ok(()),
                };

                let (sender, receiver) = mpsc::channel(100);
                let print_task = tokio::spawn(print_events(receiver));

                tx.resume(&mut session, Some(sender)).await?;

                print_task.await?;
                println!("\n");
                info!("\n\n{}", "changes applied".green().bold());
                Ok(())
            }
            Commands::Reset { step_offset } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
                let mut session = tx.load_session_cwd()?;
                tx.reset(&mut session, *step_offset)?;
                info!("Session reset to step {}", step_offset);
                Ok(())
            }
            Commands::AddCtx { files, ruskel } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
                let mut session = tx.load_session_cwd()?;

                for file in files {
                    session.add_ctx_path(file)?;
                }

                for ruskel_doc in ruskel {
                    session.add_ctx_ruskel(ruskel_doc.clone())?;
                }

                tx.save_session(&session)?;
                info!("context added");
                Ok(())
            }
            Commands::Retry {
                step_offset,
                edit,
                ctx,
                ruskel,
            } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
                let mut session = tx.load_session_cwd()?;

                let offset = step_offset.unwrap_or(session.steps().len() - 1);
                tx.reset(&mut session, offset)?;

                for file in ctx {
                    session.add_ctx_path(file)?;
                }

                for ruskel_doc in ruskel {
                    session.add_ctx_ruskel(ruskel_doc.clone())?;
                }

                if *edit {
                    match edit::edit_prompt(&session)? {
                        Some(prompt) => session.set_last_prompt(prompt)?,
                        None => return Ok(()),
                    }
                }

                let (sender, receiver) = mpsc::channel(100);
                let print_task = tokio::spawn(print_events(receiver));

                tx.resume(&mut session, Some(sender)).await?;

                print_task.await?;
                info!("\n\n{}", "changes applied".green().bold());
                Ok(())
            }
            Commands::New { files, ruskel } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
                let mut session = Session::from_cwd()?;

                for file in files {
                    session.add_ctx_path(file)?;
                }

                for ruskel_doc in ruskel {
                    session.add_ctx_ruskel(ruskel_doc.clone())?;
                }

                tx.save_session(&session)?;
                info!("new session: {}", session.root.display());
                Ok(())
            }
            Commands::Edit {
                files,
                prompt,
                prompt_file,
            } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);

                let (sender, receiver) = mpsc::channel(100);
                let print_task = tokio::spawn(print_events(receiver));

                let mut session = tx.load_session_cwd()?;

                session.add_prompt(Prompt::default())?;
                let user_prompt = if let Some(p) = prompt {
                    Prompt::User(p.clone())
                } else if let Some(file_path) = prompt_file {
                    let prompt_content =
                        fs::read_to_string(file_path).context("Failed to read prompt file")?;
                    Prompt::User(prompt_content)
                } else {
                    match edit::edit_prompt(&session)? {
                        Some(p) => p,
                        None => return Ok(()),
                    }
                };
                session.set_last_prompt(user_prompt)?;
                for f in files.clone().unwrap_or_default() {
                    session.add_editable(f)?;
                }

                tx.resume(&mut session, Some(sender)).await?;

                print_task.await?;
                println!("\n");
                info!("\n\n{}", "changes applied".green().bold());
                Ok(())
            }
            Commands::AddEdit { files } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
                let mut session = tx.load_session_cwd()?;

                for file in files {
                    session.add_editable(file)?;
                }

                tx.save_session(&session)?;
                info!("editable files added");
                Ok(())
            }
            Commands::Show { raw, render, full } => {
                let config = load_config(&cli)?;
                let model = config.model()?;
                let tx = Tenx::new(config.clone());
                let session = tx.load_session_cwd()?;
                if *raw {
                    println!("{:#?}", session);
                } else if *render {
                    println!("{}", model.render(&config, &session)?);
                } else {
                    println!("{}", pretty::session(&session, *full)?);
                }
                Ok(())
            }
        },
        None => {
            // This incredibly clunky way of doing things is because Clap is just broken when it
            // comes to specifying required subcommands in combination with Optional flags. The
            // clues to this awful situation are cunningly hidden among half a dozen issues and PRs
            // on the clap repo, e.g.
            //
            // https://github.com/clap-rs/clap/issues/5358
            //
            // In future, we can try making subcommands non-optional and removing this catchall and
            // see if this has been fixed.
            let help = Cli::command().render_help();
            println!("{help}");
            // Print help and exit
            Ok(())
        }
    }
}
