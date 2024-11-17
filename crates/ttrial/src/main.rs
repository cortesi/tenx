use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use colored::*;
use tokio::sync::mpsc;
use tracing_subscriber::util::SubscriberInitExt;

use indicatif::{ProgressBar, ProgressStyle};
use libtenx::{
    self,
    event_consumers::{self, discard_events, output_logs, output_progress},
    model::ModelProvider,
    trial::TrialReport,
};

#[derive(ValueEnum, Clone, Debug)]
enum OutputMode {
    Logs,
    Progress,
    Sum,
}

/// Prints a trial execution report in a single-line format
fn print_trial_report(report: &TrialReport) {
    let status = if report.failed {
        "fail".red()
    } else {
        "pass".green()
    };
    let errors = if report.error_patch > 0
        || report.error_validation > 0
        || report.error_response_parse > 0
        || report.error_other > 0
    {
        format!(
            " (patch:{},valid:{},parse:{},other:{})",
            report.error_patch,
            report.error_validation,
            report.error_response_parse,
            report.error_other
        )
    } else {
        String::new()
    };
    println!(
        "{} - {}: {}, {:.1}s, tokens (in/out): {}/{} {}",
        report.model_name.blue(),
        report.trial_name,
        status,
        report.time_taken,
        report.tokens_in,
        report.tokens_out,
        errors
    );
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

    /// Output mode (progress, logs, or sum)
    #[clap(long, value_enum, default_value = "sum")]
    output: OutputMode,

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
    List {
        /// Optional glob patterns to filter trials
        patterns: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let verbosity = if cli.quiet { 0 } else { cli.verbose };

    let (event_kill_tx, event_kill_rx) = mpsc::channel(1);
    let (sender, receiver) = mpsc::channel(100);
    let subscriber = event_consumers::create_tracing_subscriber(verbosity, sender.clone());
    subscriber.init();

    let event_task = match cli.output {
        OutputMode::Logs => tokio::spawn(output_logs(receiver, event_kill_rx)),
        OutputMode::Progress => tokio::spawn(output_progress(receiver, event_kill_rx, verbosity)),
        OutputMode::Sum => tokio::spawn(discard_events(receiver, event_kill_rx)),
    };

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
            let conf = trial.tenx_conf.load_env();
            trial.tenx_conf = conf;

            let progress = if matches!(cli.output, OutputMode::Sum) {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template("{msg} {spinner:.blue} ")
                        .unwrap(),
                );
                let display_name = match &model {
                    Some(m) => format!("{}: {}", m, name),
                    None => format!("{}: {}", trial.tenx_conf.model()?.name(), name),
                };
                pb.set_message(display_name);
                pb.enable_steady_tick(std::time::Duration::from_millis(100));
                Some(pb)
            } else {
                None
            };

            let report = trial.execute(Some(sender.clone()), model.clone()).await?;

            if let Some(pb) = progress {
                pb.finish();
                let status = if report.failed {
                    "fail".red()
                } else {
                    "pass".green()
                };
                println!("    {}", status);
            } else {
                print_trial_report(&report);
            }
            Ok(())
        }
        Commands::List { patterns } => {
            let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
            let pattern_slice = if pattern_refs.is_empty() {
                None
            } else {
                Some(pattern_refs.as_slice())
            };
            let trials = libtenx::trial::list(&trials_path, pattern_slice)?;
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
