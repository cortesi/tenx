use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context as AnyhowContext, Result};
use clap::{CommandFactory, Parser, Subcommand};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::to_string_pretty;
use tokio::sync::mpsc;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use libtenx::{
    self, config, dialect::DialectProvider, model::ModelProvider, prompt::Prompt, Event, LogLevel,
    Session, Tenx,
};

/// Helper function to add context to a session
/// Returns the total number of new context items added
fn add_context(
    session: &mut Session,
    config: &config::Config,
    ctx: &[String],
    ruskel: &[String],
) -> anyhow::Result<usize> {
    let mut total_added = 0;
    for file in ctx {
        total_added += session.add_ctx(config, file)?;
    }
    for ruskel_doc in ruskel {
        total_added += session.add_ctx_ruskel(ruskel_doc.clone())?;
    }
    Ok(total_added)
}

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

    /// Anthropic API key [env: ANTHROPIC_API_KEY]
    #[clap(long, global = true)]
    anthropic_key: Option<String>,

    /// Session storage directory (~/.config/tenx/state by default)
    #[clap(long, global = true)]
    session_store_dir: Option<PathBuf>,

    /// Number of times to retry a prompt before failing
    #[clap(long, global = true)]
    retry_limit: Option<usize>,

    /// Skip preflight checks
    #[clap(long, global = true)]
    no_preflight: bool,

    /// Force colored output
    #[clap(long, global = true, conflicts_with = "no_color")]
    color: bool,

    /// Disable colored output
    #[clap(long, global = true)]
    no_color: bool,

    /// Enable or disable smart mode for the Tags dialect
    #[clap(long, global = true)]
    tags_smart: Option<bool>,

    /// Enable or disable replace mode for the Tags dialect
    #[clap(long, global = true)]
    tags_replace: Option<bool>,

    /// Enable or disable udiff mode for the Tags dialect
    #[clap(long, global = true)]
    tags_udiff: Option<bool>,

    /// Enable or disable Rust Cargo Clippy validator
    #[clap(long, global = true)]
    rust_cargo_clippy: Option<bool>,

    /// Enable or disable Rust Cargo Check validator
    #[clap(long, global = true)]
    rust_cargo_check: Option<bool>,

    /// Enable or disable Rust Cargo Test validator
    #[clap(long, global = true)]
    rust_cargo_test: Option<bool>,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum DialectCommands {
    /// Print the current dialect and its settings
    Info,
    /// Print the complete system prompt
    System,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the current configuration
    Conf {
        /// Output as JSON instead of TOML
        #[clap(long)]
        json: bool,
        /// Output full configuration
        #[clap(long)]
        full: bool,
        /// Output default configuration
        #[clap(long)]
        defaults: bool,
    },
    /// List files included in the project
    LsFiles,
    /// List all validators and their status
    Validators,
    /// Dialect commands (alias: dia)
    #[clap(alias = "dia")]
    Dialect {
        #[clap(subcommand)]
        command: DialectCommands,
    },
    /// Perform an AI-assisted edit with the current session
    Edit {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Option<Vec<String>>,

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
        ctx: Vec<String>,
    },
    /// Start a new session, edit the prompt, and run it
    Oneshot {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Vec<String>,

        /// Add ruskel documentation as context
        #[clap(long)]
        ruskel: Vec<String>,

        /// Add files as context
        #[clap(long)]
        ctx: Vec<String>,
    },
    /// Start a new session and attempt to fix any preflight failures
    Fix {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Vec<String>,

        /// Add ruskel documentation as context
        #[clap(long)]
        ruskel: Vec<String>,

        /// Add files as context
        #[clap(long)]
        ctx: Vec<String>,

        /// Clear the current session, and use it to fix
        #[clap(long)]
        clear: bool,
    },
    /// Show the current session (alias: sess)
    #[clap(alias = "sess")]
    Session {
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
    /// Add context to an existing session
    AddCtx {
        /// Specifies files to add as context
        #[clap(value_parser)]
        files: Vec<String>,

        /// Add ruskel documentation
        #[clap(long)]
        ruskel: Vec<String>,
    },
    /// Add editable files to an existing session
    AddEdit {
        /// Specifies files to add as editable
        #[clap(value_parser)]
        files: Vec<String>,
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
        ctx: Vec<String>,
        /// Add ruskel documentation as context
        #[clap(long)]
        ruskel: Vec<String>,
    },
    /// Create a new session
    New {
        /// Specifies files to add as context
        #[clap(value_parser)]
        files: Vec<String>,

        /// Add ruskel documentation
        #[clap(long)]
        ruskel: Vec<String>,
    },
    /// Clear the current session
    Clear,
}

/// Creates a Config from disk and CLI arguments
fn load_config(cli: &Cli) -> Result<config::Config> {
    let mut config = config::Config::default();

    // Load from home config file
    let home_config_path = config::home_config_dir().join(config::HOME_CONFIG_FILE);
    if home_config_path.exists() {
        let home_config_str =
            fs::read_to_string(&home_config_path).context("Failed to read home config file")?;
        let home_config = config::Config::from_toml(&home_config_str)
            .context("Failed to parse home config file")?;
        config.merge(&home_config);
    }

    // Load from local config file
    let project_root = config::find_project_root(&std::env::current_dir()?);
    let local_config_path = project_root.join(config::LOCAL_CONFIG_FILE);
    if local_config_path.exists() {
        let local_config_str =
            fs::read_to_string(&local_config_path).context("Failed to read local config file")?;
        let local_config = config::Config::from_toml(&local_config_str)
            .context("Failed to parse local config file")?;
        config.merge(&local_config);
    }

    macro_rules! set_config {
    ($config:expr, $($field:ident).+, $value:expr) => {
        if let Some(val) = $value {
            $config.$($field).+ = val;
        }
    };
}

    // Apply CLI arguments
    config = config.load_env();
    set_config!(config, session_store_dir, cli.session_store_dir.clone());
    set_config!(config, anthropic_key, cli.anthropic_key.clone());
    set_config!(config, retry_limit, cli.retry_limit);
    set_config!(config, tags.smart, cli.tags_smart);
    set_config!(config, tags.replace, cli.tags_replace);
    set_config!(config, tags.udiff, cli.tags_udiff);
    config.no_preflight = cli.no_preflight;

    // Override validator configurations
    if let Some(value) = cli.rust_cargo_clippy {
        config.validators.rust_cargo_clippy = value;
    }
    if let Some(value) = cli.rust_cargo_check {
        config.validators.rust_cargo_check = value;
    }
    if let Some(value) = cli.rust_cargo_test {
        config.validators.rust_cargo_test = value;
    }

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

/// Handles events with improved progress output using indicatif
async fn progress_events(mut receiver: mpsc::Receiver<Event>) {
    let spinner_style = ProgressStyle::with_template("{spinner:.blue.bold} {msg}")
        .unwrap()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);

    let validator_spinner_style = ProgressStyle::with_template("{spinner:.blue.bold} {msg}")
        .unwrap()
        .tick_strings(&["▹▹▹▹▹", "▸▹▹▹▹", "▹▸▹▹▹", "▹▹▸▹▹", "▹▹▹▸▹", "▹▹▹▹▸"]);

    let mut current_spinner: Option<ProgressBar> = None;

    fn manage_spinner<F>(spinner: &mut Option<ProgressBar>, f: F)
    where
        F: FnOnce(&ProgressBar),
    {
        if let Some(s) = spinner.take() {
            f(&s);
        }
    }

    while let Some(event) = receiver.recv().await {
        match event {
            Event::Retry(ref message) => {
                manage_spinner(&mut current_spinner, |s| s.finish_and_clear());
                println!("{}", format!("Retrying: {}", message).yellow());
            }
            Event::Fatal(ref message) => {
                manage_spinner(&mut current_spinner, |s| s.finish_and_clear());
                println!("{}", format!("Fatal: {}", message).red());
            }
            Event::Snippet(ref chunk) => {
                manage_spinner(&mut current_spinner, |s| s.finish_and_clear());
                print!("{}", chunk);
                io::stdout().flush().unwrap();
            }
            Event::PromptDone => {
                println!("\n\n");
            }
            Event::PreflightEnd | Event::FormattingEnd | Event::ValidationEnd => {
                manage_spinner(&mut current_spinner, |s| s.finish_and_clear());
            }
            Event::FormattingOk(_) => {
                manage_spinner(&mut current_spinner, |s| s.finish_and_clear());
            }
            Event::ValidatorOk(_) => {
                manage_spinner(&mut current_spinner, |s| s.finish());
            }
            Event::Finish => {
                manage_spinner(&mut current_spinner, |s| s.finish_and_clear());
                println!("{}", "Done.".green().bold());
            }
            Event::ValidatorStart(msg) => {
                if let Some(spinner) = current_spinner.as_ref() {
                    spinner.finish();
                }
                let new_spinner =
                    ProgressBar::new_spinner().with_style(validator_spinner_style.clone());
                new_spinner.enable_steady_tick(std::time::Duration::from_millis(100));
                new_spinner.set_message(format!("  Checking: {}", msg));
                current_spinner = Some(new_spinner);
            }
            Event::Log(_, _) => {} // Ignore Log events in progress_events
            _ => {
                if let Some(msg) = event.step_start_message() {
                    if let Some(spinner) = current_spinner.as_ref() {
                        spinner.finish();
                    }
                    let new_spinner = ProgressBar::new_spinner().with_style(spinner_style.clone());
                    new_spinner.enable_steady_tick(std::time::Duration::from_millis(100));
                    new_spinner.set_message(msg);
                    current_spinner = Some(new_spinner);
                }
            }
        }
    }

    manage_spinner(&mut current_spinner, |s| s.finish_and_clear());
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let verbosity = if cli.quiet { 0 } else { cli.verbose };
    let config = load_config(&cli)?;
    let tx = Tenx::new(config.clone());

    if cli.color {
        colored::control::set_override(true);
    } else if cli.no_color {
        colored::control::set_override(false);
    }

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
            Commands::Conf {
                json,
                full,
                defaults,
            } => {
                let mut conf = if *defaults {
                    config::Config::default()
                } else {
                    config.clone()
                };
                if *full || *defaults {
                    conf = conf.with_full(true);
                }
                if *json {
                    println!("{}", to_string_pretty(&conf)?);
                } else {
                    println!("{}", conf.to_toml()?);
                }
                Ok(()) as anyhow::Result<()>
            }
            Commands::LsFiles => {
                let session = Session::from_cwd(&config)?;
                let files = config.included_files(&session.root)?;
                for file in files {
                    println!("{}", file.display());
                }
                Ok(())
            }
            Commands::Validators => {
                for validator in libtenx::all_validators() {
                    let name = validator.name();
                    let configured = validator.is_configured(&config);
                    let runnable = validator.runnable();

                    let status = if !configured {
                        format!("{} {}", "✗".yellow(), " (disabled)".yellow())
                    } else {
                        match runnable {
                            Ok(libtenx::Runnable::Ok) => "✓".green().to_string(),
                            Ok(libtenx::Runnable::Error(msg)) => {
                                format!("{} ({})", "✗".red(), msg.red())
                            }
                            Err(_) => "✗".red().to_string(),
                        }
                    };

                    println!("{:<30} {}", name, status);
                }
                Ok(())
            }
            Commands::Dialect { command } => {
                let dialect = config.dialect()?;
                match command {
                    DialectCommands::Info => {
                        println!("Current dialect: {}", dialect.name());
                        println!("\nSettings:\n{:#?}", config.tags);
                    }
                    DialectCommands::System => {
                        println!("Current dialect: {}", dialect.name());
                        println!("\nSystem prompt:\n{}", dialect.system());
                    }
                }
                Ok(())
            }
            Commands::Oneshot { files, ruskel, ctx } => {
                let mut session = Session::from_cwd(&config)?;

                add_context(&mut session, &config, ctx, ruskel)?;

                for file in files {
                    session.add_editable(&config, file)?;
                }

                session.add_prompt(Prompt::User(String::new()))?;
                match edit::edit_prompt(&session)? {
                    Some(p) => session.set_last_prompt(p)?,
                    None => return Ok(()),
                };

                tx.prompt(&mut session, Some(sender.clone())).await?;
                Ok(())
            }
            Commands::Edit {
                files,
                prompt,
                prompt_file,
                ruskel,
                ctx,
            } => {
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
                    session.add_editable(&config, &f)?;
                }

                add_context(&mut session, &config, ctx, ruskel)?;

                tx.prompt(&mut session, Some(sender)).await?;
                Ok(())
            }
            Commands::Session { raw, render, full } => {
                let model = config.model()?;
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
            Commands::AddCtx { files, ruskel } => {
                let mut session = tx.load_session_cwd()?;

                let added = add_context(&mut session, &config, files, ruskel)?;

                if added == 0 {
                    return Err(anyhow::anyhow!("No new context items were added"));
                }

                tx.save_session(&session)?;
                println!("{} new context item(s) added", added);
                Ok(())
            }
            Commands::AddEdit { files } => {
                let mut session = tx.load_session_cwd()?;

                let mut total_added = 0;
                for file in files {
                    total_added += session.add_editable(&config, file)?;
                }

                if total_added == 0 {
                    return Err(anyhow::anyhow!("No new editable files were added"));
                }

                tx.save_session(&session)?;
                println!("{} new editable file(s) added", total_added);
                Ok(())
            }
            Commands::Reset { step_offset } => {
                let mut session = tx.load_session_cwd()?;
                tx.reset(&mut session, *step_offset)?;
                println!("Session reset to step {}", step_offset);
                Ok(())
            }
            Commands::Retry {
                step_offset,
                edit,
                ctx,
                ruskel,
            } => {
                let mut session = tx.load_session_cwd()?;

                let offset = step_offset.unwrap_or(session.steps().len() - 1);
                tx.reset(&mut session, offset)?;

                add_context(&mut session, &config, ctx, ruskel)?;

                if *edit {
                    match edit::edit_prompt(&session)? {
                        Some(prompt) => session.set_last_prompt(prompt)?,
                        None => return Ok(()),
                    }
                }

                tx.retry(&mut session, Some(sender.clone())).await?;
                Ok(())
            }
            Commands::New { files, ruskel } => {
                let mut session = Session::from_cwd(&config)?;

                add_context(&mut session, &config, files, ruskel)?;

                tx.save_session(&session)?;
                println!("new session: {}", session.root.display());
                Ok(())
            }
            Commands::Fix {
                files,
                ruskel,
                ctx,
                clear,
            } => {
                let mut session = if *clear {
                    let mut current_session = tx.load_session_cwd()?;
                    current_session.clear();
                    current_session
                } else {
                    Session::from_cwd(&config)?
                };

                for file in files {
                    session.add_editable(&config, file)?;
                }

                add_context(&mut session, &config, ctx, ruskel)?;

                tx.fix(&mut session, Some(sender.clone())).await?;
                if session.pending_prompt() {
                    tx.prompt(&mut session, Some(sender.clone())).await?;
                } else {
                    println!("No issues to fix");
                }
                tx.save_session(&session)?;
                Ok(())
            }
            Commands::Clear => {
                let mut session = tx.load_session_cwd()?;
                session.clear();
                tx.save_session(&session)?;
                println!("Session cleared");
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
