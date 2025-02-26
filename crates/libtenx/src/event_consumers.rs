//! Helpers for consuming and displaying events
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;
use textwrap;
use tokio::sync::mpsc;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{fmt, EnvFilter};

use crate::events::{Event, EventReceiver, EventSender, LogLevel};

const SPINNER_STRINGS: &[&str] = &["▹▹▹▹▹", "▸▹▹▹▹", "▹▸▹▹▹", "▹▹▸▹▹", "▹▹▹▸▹", "▹▹▹▹▸"];

/// Discards all events without processing them
pub async fn discard_events(mut receiver: EventReceiver, mut kill_signal: mpsc::Receiver<()>) {
    loop {
        tokio::select! {
            _ = receiver.recv() => {}
            _ = kill_signal.recv() => break,
            else => break,
        }
    }
}

/// Creates a subscriber that sends all tracing events to an mpsc channel for processing.
pub fn create_tracing_subscriber(verbosity: u8, sender: EventSender) -> impl Subscriber {
    let filter = match verbosity {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        3 => EnvFilter::new("trace"),
        _ => EnvFilter::new("trace"),
    };

    struct Writer {
        sender: EventSender,
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

/// Output events in a text log format
pub async fn output_logs(mut receiver: EventReceiver, mut kill_signal: mpsc::Receiver<()>) {
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
                    Event::IterationLimit => {
                        println!("{}: step limit reached", "warn".yellow());
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

/// Fancy event output, with progress bars
pub async fn output_progress(
    mut receiver: EventReceiver,
    mut kill_signal: mpsc::Receiver<()>,
    verbosity: u8,
) {
    let spinner_indent = SPINNER_STRINGS[0].chars().count();
    let validator_spinner_style = ProgressStyle::with_template("    {spinner:.green.bold} {msg}")
        .unwrap()
        .tick_strings(SPINNER_STRINGS);

    let mut current_spinner: Option<ProgressBar> = None;

    fn finish_spinner(spinner: &mut Option<ProgressBar>) {
        if let Some(s) = spinner.take() {
            s.finish();
        }
    }

    fn start_new_spinner(
        current_spinner: &mut Option<ProgressBar>,
        style: &ProgressStyle,
        message: &str,
    ) {
        if let Some(spinner) = current_spinner.as_ref() {
            spinner.finish();
        }
        let new_spinner = ProgressBar::new_spinner().with_style(style.clone());
        new_spinner.enable_steady_tick(Duration::from_millis(100));
        new_spinner.set_message(message.to_string());
        *current_spinner = Some(new_spinner);
    }

    loop {
        tokio::select! {
            Some(event) = receiver.recv() => {
                if let Some(header) = event.header_message() {
                    finish_spinner(&mut current_spinner);
                    println!("{}", header.blue());
                } else if let Some(progress_event) = event.progress_event() {
                    start_new_spinner(
                        &mut current_spinner,
                        &validator_spinner_style,
                        &progress_event,
                    );
                }

                match event {
                    Event::Throttled(ms) => {
                        finish_spinner(&mut current_spinner);
                        println!("{:>width$}{}", "", format!("throttled: waiting {}ms", ms).yellow(), width=spinner_indent);
                    }
                    Event::Interact => {
                        finish_spinner(&mut current_spinner);
                        println!("{}", "getting user input...".blue());
                    }
                    Event::NextStep{ref user, ref model} => {
                        finish_spinner(&mut current_spinner);
                        println!("{:>width$}{}", "", format!("next step: {}", user).yellow(), width=spinner_indent);
                        if verbosity > 0 {
                            let wrapped = textwrap::indent(
                                &textwrap::fill(model, 80 - spinner_indent),
                                &" ".repeat(spinner_indent)
                            );
                            println!("{:>width$}Model message:", "", width=spinner_indent);
                            println!("{}", wrapped.yellow());
                        }
                    }
                    Event::IterationLimit => {
                        finish_spinner(&mut current_spinner);
                        println!("{:>width$}{}", "", "step limit reached".yellow(), width=spinner_indent);
                    }
                    Event::Fatal(ref message) => {
                        finish_spinner(&mut current_spinner);
                        println!("{:>width$}{}", "", format!("fatal: {}", message).red(), width=spinner_indent);
                    }
                    Event::Snippet(ref chunk) => {
                        finish_spinner(&mut current_spinner);
                        print!("{}", chunk);
                    }
                    Event::ModelResponse(ref text) => {
                        finish_spinner(&mut current_spinner);
                        print!("{}", text);
                    }
                    Event::Finish => {
                        finish_spinner(&mut current_spinner);
                    }
                    Event::PromptEnd(_) => {
                        finish_spinner(&mut current_spinner);
                        println!("\n");
                    }
                    _ => {}
                }
            }
            _ = kill_signal.recv() => break,
            else => break,
        }
    }

    finish_spinner(&mut current_spinner);
}
