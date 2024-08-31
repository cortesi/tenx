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
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use libtenx::{self, config, model::ModelProvider, prompt::Prompt, Event, LogLevel, Session, Tenx};

mod edit;
mod pretty;

/// Creates a subscriber that sends events to an mpsc channel.
fn create_subscriber(verbosity: u8, sender: mpsc::Sender<Event>) -> impl Subscriber {
    let filter = match verbosity {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        3 => EnvFilter::new("trace"),
        _ => EnvFilter::new("warn"),
    };

    struct Writer {
        sender: mpsc::Sender<Event>,
    }

    impl std::io::Write for Writer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Ok(s) = std::str::from_utf8(buf) {
                let _ = self
                    .sender
                    .try_send(Event::Log(LogLevel::Info, s.to_string()));
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let make_writer = move || Writer {
        sender: sender.clone(),
    };

    fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_writer(make_writer)
        .with_span_events(FmtSpan::NONE)
        .without_time()
        .finish()
}

#[derive(Parser)]
#[clap(name = "tenx")]
#[clap(author = "Aldo Cortesi")]
#[clap(version = "0.1.0")]
#[clap(about = "AI-powered coding assistant", long_about = None)]
struct Cli {
    /// Increase output verbosity
    #[clap(short, long, action = clap::ArgAction::Count, global = true, default_value = "0")]
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

        /// Add ruskel documentation as context
        #[clap(long)]
        ruskel: Vec<String>,

        /// Add files as context
        #[clap(long)]
        ctx: Vec<PathBuf>,
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
            Event::Log(level, message) => {
                let severity = match level {
                    LogLevel::Error => "error".red(),
                    LogLevel::Warn => "warn".yellow(),
                    LogLevel::Info => "info".green(),
                    LogLevel::Debug => "debug".cyan(),
                    LogLevel::Trace => "trace".magenta(),
                };
                println!("{}: {}", severity, message);
            }
            _ => {
                let name = event.name().to_string();
                let display = event.display();
                if display.is_empty() {
                    println!("{}", name.blue());
                } else {
                    println!("{}: {}", name.blue(), display);
                }
            }
        }
    }
}

/// Handles events with minimal progress output
async fn progress_events(mut receiver: mpsc::Receiver<Event>) {
    while let Some(event) = receiver.recv().await {
        match event {
            Event::Snippet(chunk) => {
                print!("{}", chunk);
                io::stdout().flush().unwrap();
            }
            Event::PreflightStart => println!("{}", "Starting preflight checks...".blue()),
            Event::PreflightEnd => println!("{}", "Preflight checks completed.".blue()),
            Event::FormattingStart => println!("{}", "Starting formatting...".blue()),
            Event::FormattingEnd => println!("{}", "Formatting completed.".blue()),
            Event::FormattingOk(name) => println!(
                "\t{} {}",
                format!("'{}' completed.", name).green(),
                "✓".green()
            ),
            Event::ValidationStart => println!("{}", "Starting post-patch validation...".blue()),
            Event::ValidationEnd => println!("{}", "Post-patch validation completed.".blue()),
            Event::CheckStart(name) => print!("\t{}...", name),
            Event::CheckOk(_) => println!(" done"),
            Event::Log(_, _) => {} // Ignore Log events in progress_events
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let verbosity = if cli.quiet { 0 } else { cli.verbose };

    let (sender, receiver) = mpsc::channel(100);
    let (event_kill_tx, mut event_kill_rx) = mpsc::channel(1);
    let subscriber = create_subscriber(verbosity, sender.clone());
    subscriber.init();
    let event_task = if verbosity > 0 {
        tokio::spawn(async move {
            tokio::select! {
                _ = print_events(receiver) => {},
                _ = event_kill_rx.recv() => {},
            }
        })
    } else {
        tokio::spawn(async move {
            tokio::select! {
                _ = progress_events(receiver) => {},
                _ = event_kill_rx.recv() => {},
            }
        })
    };

    let result = match &cli.command {
        Some(cmd) => match cmd {
            Commands::Conf => {
                let config = load_config(&cli)?;
                println!("{}", to_string_pretty(&config)?);
                Ok(()) as anyhow::Result<()>
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

                tx.resume(&mut session, Some(sender.clone())).await?;
                println!("{}", "changes applied".green().bold());
                Ok(())
            }
            Commands::Reset { step_offset } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
                let mut session = tx.load_session_cwd()?;
                tx.reset(&mut session, *step_offset)?;
                println!("Session reset to step {}", step_offset);
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
                println!("context added");
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

                tx.resume(&mut session, Some(sender.clone())).await?;
                println!("\n\n{}", "changes applied".green().bold());
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
                println!("new session: {}", session.root.display());
                Ok(())
            }
            Commands::Edit {
                files,
                prompt,
                prompt_file,
                ruskel,
                ctx,
            } => {
                let config = load_config(&cli)?;
                let tx = Tenx::new(config);
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

                for file in ctx {
                    session.add_ctx_path(file)?;
                }

                for ruskel_doc in ruskel {
                    session.add_ctx_ruskel(ruskel_doc.clone())?;
                }

                tx.resume(&mut session, Some(sender)).await?;

                println!("\n");
                println!("\n\n{}", "changes applied".green().bold());
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
                println!("editable files added");
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
    };

    result?;

    // Signal the event task to terminate
    let _ = event_kill_tx.send(()).await;
    // Wait for the event task to finish
    let _ = tokio::time::timeout(std::time::Duration::from_millis(100), event_task).await;

    Ok(())
}
