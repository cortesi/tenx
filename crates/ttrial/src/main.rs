use std::path::PathBuf;
use textwrap::dedent;

use clap::{Parser, Subcommand, ValueEnum};
use colored::*;
use tokio::sync::mpsc;
use tracing_subscriber::util::SubscriberInitExt;

use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};
use indicatif::{ProgressBar, ProgressStyle};
use libtenx::{
    self,
    event_consumers::{self, discard_events, output_logs, output_progress},
    pretty,
    session_store::SessionStore,
    Event, Session,
};
use libttrial::*;

#[derive(ValueEnum, Clone, Debug)]
enum OutputMode {
    Logs,
    Progress,
    Sum,
}

#[derive(ValueEnum, Clone, Debug)]
enum ReportFormat {
    Text,
    Table,
}

/// Run a single trial and return its report
async fn run_trial(
    trial: &mut Trial,
    output_mode: &OutputMode,
    sender: &mpsc::Sender<Event>,
    model_name: &str,
    iteration: usize,
) -> anyhow::Result<(TrialReport, Session)> {
    trial.tenx_conf = trial.tenx_conf.clone().load_env();

    let progress = if matches!(output_mode, OutputMode::Sum) {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{msg} {spinner:.blue} ")
                .unwrap(),
        );
        let display_name = format!("{}: {}", model_name, trial.name);
        pb.set_message(display_name);
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    let session = trial.execute(Some(sender.clone()), model_name).await?;
    let report = TrialReport::from_session(&session, &trial.name, iteration, &trial.tenx_conf)?;

    if let Some(pb) = progress {
        pb.finish();
        let status = if report.failed {
            "fail".red()
        } else {
            "pass".green()
        };
        println!("    {}", status);
    }

    Ok((report, session))
}

/// Sorts trial reports by model name first, then by trial name
fn sort_reports(reports: &mut [TrialReport]) {
    reports.sort_by(|a, b| {
        a.model_name
            .cmp(&b.model_name)
            .then(a.trial_name.cmp(&b.trial_name))
            .then(a.n.cmp(&b.n))
    });
}

/// Prints trial execution reports in a text format
fn print_report_text(reports: &mut [TrialReport]) {
    sort_reports(reports);
    for report in reports {
        println!("{:#?}", report);
    }
}

/// Prints trial execution reports in a table format
fn print_report_table(reports: &mut [TrialReport]) {
    sort_reports(reports);
    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(vec![
        Cell::new("model"),
        Cell::new("trial"),
        Cell::new("n"),
        Cell::new("status"),
        Cell::new("steps"),
        Cell::new("time (s)"),
        Cell::new("words recv"),
        Cell::new("errors"),
    ]);

    for report in reports {
        let status = if report.failed { "fail" } else { "pass" };

        let mut errors = Vec::new();
        if report.error_check > 0 {
            errors.push(format!("check: {}", report.error_check));
        }
        if report.error_patch > 0 {
            errors.push(format!("patch: {}", report.error_patch));
        }
        if report.error_response_parse > 0 {
            errors.push(format!("parse: {}", report.error_response_parse));
        }
        if report.error_other > 0 {
            errors.push(format!("other: {}", report.error_other));
        }
        let errors = errors.join("\n");

        table.add_row(vec![
            Cell::new(&report.model_name),
            Cell::new(&report.trial_name),
            Cell::new(report.n.to_string()),
            Cell::new(status).fg(if report.failed {
                Color::Red
            } else {
                Color::Green
            }),
            Cell::new(report.steps.to_string()),
            Cell::new(format!("{:.1}", report.total_response_time)),
            Cell::new(report.words_received.to_string()),
            Cell::new(errors),
        ]);
    }

    println!("{table}");
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
    /// Run trials matching patterns
    Run {
        /// Optional glob patterns to filter trials
        patterns: Vec<String>,

        /// Report format (text or table)
        #[clap(long, value_enum, default_value = "table")]
        report: ReportFormat,

        /// Override the models to use (can be specified multiple times)
        #[clap(long, num_args = 1)]
        model: Vec<String>,

        /// Directory to save all trial sessions to
        #[clap(long)]
        save: Option<PathBuf>,

        /// Skip printing the report
        #[clap(long)]
        no_report: bool,

        /// Print detailed session information
        #[clap(long)]
        session: bool,

        /// Resume trials by skipping those with existing saved sessions
        #[clap(long, requires = "save")]
        resume: bool,

        /// Number of times to run each trial
        #[clap(short = 'n', long, default_value = "1")]
        iterations: usize,
    },
    /// List all available trials (alias: ls)
    #[clap(alias = "ls")]
    List {
        /// Optional glob patterns to filter trials
        patterns: Vec<String>,
    },
    /// Generate a report from stored sessions
    Report {
        /// Directory containing stored sessions
        store: PathBuf,
    },
    /// Generate aggregate model scores from stored sessions
    Score {
        /// Directory containing stored sessions
        store: PathBuf,
    },
}

/// Format a trial description by dedenting, reflowing, and trimming whitespace
fn format_description(desc: &str) -> String {
    let dedented = dedent(desc);
    let trimmed = dedented.trim();
    textwrap::fill(trimmed, 72)
}

/// Format a session name from components using colons as separators
fn format_session_name(model: &str, trial: &str, iteration: usize) -> String {
    format!("{}:{}:{}", model, trial, iteration + 1)
}

/// Parse a session name into its components, expecting colon separators
fn parse_session_name(session_name: &str) -> Option<(&str, &str, usize)> {
    let mut parts = session_name.rsplitn(2, ':');
    let iteration = parts.next()?.parse::<usize>().ok()?.checked_sub(1)?;
    let remainder = parts.next()?;
    let parts = remainder.split_once(':')?;
    Some((parts.0, parts.1, iteration))
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

    /// Prints model scores in a table format
    fn print_score_table(scores: &[ModelScore]) {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL).set_header(vec![
            Cell::new("model"),
            Cell::new("api model"),
            Cell::new("trials"),
            Cell::new("success %"),
            Cell::new("errors"),
            Cell::new("time (s)"),
            Cell::new("words"),
        ]);

        for score in scores {
            let mut errors = Vec::new();
            if score.error_check > 0 {
                errors.push(format!("check: {}", score.error_check));
            }
            if score.error_patch > 0 {
                errors.push(format!("patch: {}", score.error_patch));
            }
            if score.error_response_parse > 0 {
                errors.push(format!("parse: {}", score.error_response_parse));
            }
            if score.error_other > 0 {
                errors.push(format!("other: {}", score.error_other));
            }
            let errors = errors.join("\n");

            let success_rate = if score.total_trials > 0 {
                (score.total_succeeds as f64 / score.total_trials as f64) * 100.0
            } else {
                0.0
            };

            table.add_row(vec![
                Cell::new(&score.model_name),
                Cell::new(&score.api_model),
                Cell::new(score.total_trials.to_string()),
                Cell::new(format!("{:.1}%", success_rate)),
                Cell::new(errors),
                Cell::new(format!("{:.1}", score.total_time)),
                Cell::new(score.total_words.to_string()),
            ]);
        }

        println!("{table}");
    }

    let result = match cli.command {
        Commands::Report { store } => {
            let store = SessionStore::open(store)?;
            let sessions = store.list()?;
            let mut reports = Vec::new();

            for session_name in sessions {
                let (_, trial_name, iteration) = parse_session_name(&session_name)
                    .ok_or_else(|| anyhow::anyhow!("Invalid session name: {}", session_name))?;

                let session = store.load(session_name.clone())?;
                let report = TrialReport::from_session(
                    &session,
                    trial_name,
                    iteration,
                    &libtenx::config::load_config()?,
                )?;
                reports.push(report);
            }

            print_report_table(&mut reports);
            Ok(())
        }
        Commands::Run {
            patterns,
            report,
            model,
            save,
            resume,
            no_report,
            session: session_flag,
            iterations,
        } => {
            let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
            let pattern_slice = if pattern_refs.is_empty() {
                None
            } else {
                Some(pattern_refs.as_slice())
            };
            let mut trials = list_trials(&trials_path, pattern_slice)?;

            if trials.is_empty() {
                return Err(anyhow::anyhow!("No trials found matching patterns"));
            }

            let mut reports = Vec::new();
            let models = if model.is_empty() {
                vec!["sonnet".to_string()]
            } else {
                model.to_vec()
            };
            let session_store = if let Some(save_dir) = &save {
                Some(SessionStore::open(save_dir.clone())?)
            } else {
                None
            };

            for model in models {
                for trial in &mut trials {
                    for i in 0..iterations {
                        let session_name = format_session_name(&model, &trial.name, i);
                        if resume {
                            if let Some(store) = &session_store {
                                if store.list()?.contains(&session_name) {
                                    if matches!(cli.output, OutputMode::Sum) {
                                        println!(
                                            "{}: {} ({})\n    {}",
                                            model,
                                            trial.name,
                                            i + 1,
                                            "skip".yellow()
                                        );
                                    }
                                    continue;
                                }
                            }
                        }

                        let (report, session) =
                            run_trial(trial, &cli.output, &sender, &model, i).await?;

                        if let Some(store) = &session_store {
                            store.save(&session_name, &session)?;
                        }
                        if session_flag {
                            println!("\n{}", "-".repeat(80));
                            println!("Session for {} - {}:", report.model_name.blue(), trial.name);
                            println!(
                                "{}",
                                pretty::print_session(&trial.tenx_conf, &session, true)?
                            );
                        }
                        reports.push(report);
                    }
                }
            }

            if !no_report && !reports.is_empty() {
                match report {
                    ReportFormat::Text => print_report_text(&mut reports),
                    ReportFormat::Table => print_report_table(&mut reports),
                }

                println!("\nSummary:");
                let total = reports.len();
                let failed = reports.iter().filter(|r| r.failed).count();
                let total_time: f64 = reports.iter().map(|r| r.total_response_time).sum();

                println!(
                    "Ran {} trials in {:.1}s ({} failed)",
                    total, total_time, failed
                );
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
            let trials = list_trials(&trials_path, pattern_slice)?;
            for trial in trials {
                println!("{}", trial.name.blue().bold());
                if !trial.desc.is_empty() {
                    let desc = format_description(&trial.desc);
                    for line in desc.lines() {
                        println!("    {}", line);
                    }
                }
            }
            Ok(())
        }
        Commands::Score { store } => {
            let store = SessionStore::open(store)?;
            let sessions = store.list()?;
            let mut reports = Vec::new();

            for session_name in sessions {
                let (_, trial_name, iteration) = parse_session_name(&session_name)
                    .ok_or_else(|| anyhow::anyhow!("Invalid session name: {}", session_name))?;

                let session = store.load(session_name.clone())?;
                let report = TrialReport::from_session(
                    &session,
                    trial_name,
                    iteration,
                    &libtenx::config::load_config()?,
                )?;
                reports.push(report);
            }

            let scores = model_scores(&reports);
            print_score_table(&scores);
            Ok(())
        }
    };

    // Wait for the event task to finish
    let _ = event_kill_tx.send(()).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), event_task).await;

    result
}
