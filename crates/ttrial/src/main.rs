use anyhow::{Context as AnyhowContext, Result};
use clap::{Parser, Subcommand};
use colored::*;
use std::{fs, path::PathBuf};
use tokio::sync::mpsc;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use libtenx::{self, config, Event, LogLevel};

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
#[clap(name = "ttrial")]
#[clap(author = "Aldo Cortesi")]
#[clap(version = "0.1.0")]
#[clap(max_term_width = 80)]
#[clap(about = "AI-powered coding assistant trial runner", long_about = None)]
struct Cli {
    /// Increase output verbosity
    #[clap(short, long, action = clap::ArgAction::Count, default_value = "0")]
    verbose: u8,

    /// Decrease output verbosity
    #[clap(short, long)]
    quiet: bool,

    /// Show raw log output instead of progress indicators
    #[clap(long)]
    logs: bool,

    /// Anthropic API key [env: ANTHROPIC_API_KEY]
    #[clap(long)]
    anthropic_key: Option<String>,

    /// Path to trials directory
    #[clap(long)]
    trials: Option<PathBuf>,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a trial
    Run {
        /// Name of the trial to run
        name: String,

        /// Override the model to use
        #[clap(long)]
        model: Option<String>,
    },
    /// List all available trials (alias: ls)
    #[clap(alias = "ls")]
    List,
}

/// Output events in a text log format
async fn output_logs(
    mut receiver: mpsc::Receiver<Event>,
    mut kill_signal: mpsc::Receiver<()>,
    _verbosity: u8,
) {
    loop {
        tokio::select! {
            Some(event) = receiver.recv() => {
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
            _ = kill_signal.recv() => break,
            else => break,
        }
    }
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

    // Apply CLI arguments
    config = config.load_env();
    if let Some(key) = &cli.anthropic_key {
        config.anthropic_key = key.clone();
    }

    Ok(config)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let verbosity = if cli.quiet { 0 } else { cli.verbose };

    let (sender, receiver) = mpsc::channel(100);
    let (event_kill_tx, event_kill_rx) = mpsc::channel(1);
    let subscriber = create_subscriber(verbosity, sender.clone());
    subscriber.init();
    let event_task = tokio::spawn(output_logs(receiver, event_kill_rx, verbosity));

    let trials_path = if let Some(p) = cli.trials {
        p
    } else {
        let current_dir = std::env::current_dir()?;
        if current_dir.join(".git").exists() {
            current_dir.join("trials")
        } else {
            return Err(anyhow::anyhow!(
                "No trials directory specified and not in tenx repository"
            ));
        }
    };

    if !trials_path.exists() {
        return Err(anyhow::anyhow!(
            "Trials directory does not exist: {}",
            trials_path.display()
        ));
    }

    let result = match cli.command {
        Commands::Run { name, model } => {
            let mut trial = libtenx::trial::Trial::load(&trials_path, &name)?;
            let mut conf = trial.tenx_conf.load_env();
            if let Some(key) = cli.anthropic_key {
                conf.anthropic_key = key;
            }
            trial.tenx_conf = conf;
            trial.execute(Some(sender.clone()), model.clone()).await?;
            Ok(())
        }
        Commands::List => {
            let trials = libtenx::trial::list(&trials_path)?;
            for trial in trials {
                println!("{}", trial.name.blue().bold());
                if !trial.desc.is_empty() {
                    let desc = textwrap::fill(&trial.desc, 72);
                    for line in desc.lines() {
                        println!("    {}", line);
                    }
                }
            }
            Ok(())
        }
    };

    // Wait for the event task to finish
    let _ = event_kill_tx.send(()).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), event_task).await;

    result
}
