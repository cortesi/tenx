use std::{fs, path::PathBuf};

use anyhow::{Context as AnyhowContext, Result};
use clap::{CommandFactory, Parser, Subcommand};
use colored::*;
use libtenx::{
    self,
    config::{self},
    dialect::DialectProvider,
    event_consumers,
    model::ModelProvider,
    Session, Tenx,
};
use serde_json::to_string_pretty;
use tokio::sync::mpsc;
use tracing_subscriber::util::SubscriberInitExt;

mod edit;
mod pretty;

/// Gets the user's prompt from arguments or editor
fn get_prompt(
    prompt: &Option<String>,
    prompt_file: &Option<PathBuf>,
    session: &Session,
    retry: bool,
) -> Result<Option<String>> {
    if let Some(p) = prompt {
        Ok(Some(p.clone()))
    } else if let Some(file_path) = prompt_file {
        let prompt_content = fs::read_to_string(file_path).context("Failed to read prompt file")?;
        Ok(Some(prompt_content))
    } else {
        Ok(edit::edit_prompt(session, retry)?)
    }
}

#[derive(Parser)]
#[clap(name = "tenx")]
#[clap(author = "Aldo Cortesi")]
#[clap(version = "0.1.0")]
#[clap(max_term_width = 80)]
#[clap(about = "AI-powered coding assistant", long_about = None)]
struct Cli {
    /// Increase output verbosity
    #[clap(short, long, action = clap::ArgAction::Count,  default_value = "0")]
    verbose: u8,

    /// Decrease output verbosity
    #[clap(short, long)]
    quiet: bool,

    /// Show raw log output instead of progress indicators
    #[clap(long)]
    logs: bool,

    /// Model to use (overrides default_model in config)
    #[clap(long)]
    model: Option<String>,

    /// Session storage directory (~/.config/tenx/state by default)
    #[clap(long)]
    session_store_dir: Option<PathBuf>,

    /// Number of times to retry a prompt before failing
    #[clap(long)]
    retry_limit: Option<usize>,

    /// Skip pre checks
    #[clap(long)]
    no_pre_check: bool,

    /// Only run this check
    #[clap(long)]
    only_check: Option<String>,

    /// Disable streaming model output
    #[clap(long)]
    no_stream: bool,

    /// Force colored output
    #[clap(long, conflicts_with = "no_color")]
    color: bool,

    /// Disable colored output
    #[clap(long)]
    no_color: bool,

    /// Smart mode for the Tags dialect
    #[clap(long)]
    tags_smart: Option<bool>,

    /// Replace mode for the Tags dialect
    #[clap(long)]
    tags_replace: Option<bool>,

    /// Udiff mode for the Tags dialect
    #[clap(long)]
    tags_udiff: Option<bool>,

    /// Enable a specific check
    #[clap(long)]
    check: Vec<String>,

    /// Disable a specific check
    #[clap(long)]
    no_check: Vec<String>,

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
    /// Run check suite on the current session
    Check,
    /// List validators and their status
    Checks {
        /// Show all checks, including disabled
        #[clap(long)]
        all: bool,
    },
    /// Clear the current session without resetting changes
    Clear,
    /// Make an AI-assisted change using the current session
    Code {
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
        /// Add URLs as context
        #[clap(long)]
        url: Vec<String>,
    },
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
    /// Add items to context (alias: ctx)
    #[clap(alias = "ctx")]
    Context {
        /// Add items as ruskel documentation modules
        #[clap(long, group = "type")]
        ruskel: bool,
        /// Add items as files
        #[clap(long, group = "type")]
        file: bool,
        /// Add URLs as context
        #[clap(long, group = "type")]
        url: bool,
        /// Items to add to context
        items: Vec<String>,
    },
    /// Dialect commands (alias: dia)
    #[clap(alias = "dia")]
    Dialect {
        #[clap(subcommand)]
        command: DialectCommands,
    },
    /// Add editable files to a session
    Edit {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Vec<String>,
    },
    /// List files included in the project
    Files {
        /// Optional glob pattern to filter files
        pattern: Option<String>,
    },
    /// Start a new session and attempt to fix any pre check failures
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
        /// Add URLs as context
        #[clap(long)]
        url: Vec<String>,
        /// Clear the current session, and use it to fix
        #[clap(long)]
        clear: bool,
        /// User prompt for the fix operation
        #[clap(long)]
        prompt: Option<String>,
        /// Path to a file containing the prompt
        #[clap(long)]
        prompt_file: Option<PathBuf>,
        /// Edit the prompt before fixing
        #[clap(long)]
        edit: bool,
    },
    /// List configured models (alias: ls)
    #[clap(alias = "ls")]
    Models {
        /// Show full configuration details
        #[clap(short, long)]
        full: bool,
    },
    /// Create a new session
    New {
        /// Specifies files to add as context
        #[clap(value_parser)]
        files: Vec<String>,
        /// Add ruskel documentation
        #[clap(long)]
        ruskel: Vec<String>,
        /// Add URLs as context
        #[clap(long)]
        url: Vec<String>,
    },
    /// Print information about the current project
    Project,
    /// Start a new session, edit the prompt, and run it
    Quick {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Vec<String>,
        /// Add ruskel documentation as context
        #[clap(long)]
        ruskel: Vec<String>,
        /// Add files as context
        #[clap(long)]
        ctx: Vec<String>,
        /// Add URLs as context
        #[clap(long)]
        url: Vec<String>,
        /// User prompt for the edit operation
        #[clap(long)]
        prompt: Option<String>,
        /// Path to a file containing the prompt
        #[clap(long)]
        prompt_file: Option<PathBuf>,
    },
    /// Refresh all contexts in the current session
    Refresh,
    /// Reset the session to a specific step, undoing changes
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
        /// Add URLs as context
        #[clap(long)]
        url: Vec<String>,
        /// User prompt for the edit operation
        #[clap(long)]
        prompt: Option<String>,
        /// Path to a file containing the prompt
        #[clap(long)]
        prompt_file: Option<PathBuf>,
    },
    /// Show the current session (alias: sess)
    #[clap(alias = "sess")]
    Session {
        /// Path to a session file to load
        session_file: Option<PathBuf>,
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

/// Creates a Config from disk and CLI arguments
fn load_config(cli: &Cli) -> Result<config::Config> {
    let mut config = config::load_config()?;

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
    set_config!(config, retry_limit, cli.retry_limit);
    set_config!(config, tags.smart, cli.tags_smart);
    set_config!(config, tags.replace, cli.tags_replace);
    set_config!(config, tags.udiff, cli.tags_udiff);
    if let Some(model) = &cli.model {
        config.default_model = Some(model.clone());
    }
    config.checks.no_pre = cli.no_pre_check;
    config.checks.only = cli.only_check.clone();
    config.no_stream = cli.no_stream;

    // Validate checks
    if let Some(name) = &cli.only_check {
        if config.get_check(name).is_none() {
            return Err(anyhow::anyhow!("check '{}' does not exist", name));
        }
    }
    for check_name in &cli.check {
        if config.get_check(check_name).is_none() {
            return Err(anyhow::anyhow!("check '{}' does not exist", check_name));
        }
    }
    config.checks.enable.extend(cli.check.clone());

    // Validate and add disabled checks
    for check_name in &cli.no_check {
        if config.get_check(check_name).is_none() {
            return Err(anyhow::anyhow!("check '{}' does not exist", check_name));
        }
    }
    config.checks.disable.extend(cli.no_check.clone());

    Ok(config)
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
    let (event_kill_tx, event_kill_rx) = mpsc::channel(1);
    let subscriber = event_consumers::create_tracing_subscriber(verbosity, sender.clone());
    subscriber.init();
    let event_task = if cli.logs {
        tokio::spawn(event_consumers::output_logs(receiver, event_kill_rx))
    } else {
        tokio::spawn(event_consumers::output_progress(
            receiver,
            event_kill_rx,
            verbosity,
        ))
    };

    let result = match &cli.command {
        Some(cmd) => match cmd {
            Commands::Models { full } => {
                for model in &config.model_confs {
                    match model {
                        libtenx::config::ModelConfig::Claude { .. }
                        | libtenx::config::ModelConfig::OpenAi { .. } => {
                            println!("{}", model.name().blue().bold());
                            println!("    kind: {}", model.kind());
                            for line in model.text_config(*full).lines() {
                                println!("    {}", line);
                            }
                            println!();
                        }
                    }
                }
                Ok(())
            }
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
                    println!("{}", conf.to_ron()?);
                }
                Ok(()) as anyhow::Result<()>
            }
            Commands::Project => {
                let project_root = config.project_root();
                println!(
                    "{} {}",
                    "project root:".blue().bold(),
                    project_root.display()
                );
                println!("{} {}", "include strategy:".blue().bold(), config.include);
                Ok(())
            }
            Commands::Files { pattern } => {
                let files = if let Some(p) = pattern {
                    config.match_files_with_glob(p)?
                } else {
                    config.included_files()?
                };
                for file in files {
                    println!("{}", file.display());
                }
                Ok(())
            }
            Commands::Checks { all } => {
                let checks = if *all {
                    config.all_checks()
                } else {
                    config.enabled_checks()
                };
                for check in checks {
                    let name = &check.name;
                    let enabled = config.is_check_enabled(name);

                    let status = if !enabled {
                        " (disabled)".yellow().to_string()
                    } else {
                        String::new()
                    };

                    println!("{}{}", name.blue().bold(), status);
                    println!("    globs: {:?}", check.globs);
                    println!("    pre: {}", check.mode.is_pre());
                    println!("    post: {}", check.mode.is_post());
                    println!();
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
            Commands::Quick {
                files,
                ruskel,
                ctx,
                url,
                prompt,
                prompt_file,
            } => {
                let mut session = tx.new_session_from_cwd(&Some(sender.clone())).await?;
                tx.add_contexts(&mut session, ctx, ruskel, url, false, &Some(sender.clone()))
                    .await?;
                for file in files {
                    session.add_editable(&config, file)?;
                }

                let user_prompt = match get_prompt(prompt, prompt_file, &session, false)? {
                    Some(p) => p,
                    None => return Ok(()),
                };
                tx.code(&mut session, user_prompt, Some(sender.clone()))
                    .await?;
                Ok(())
            }
            Commands::Code {
                files,
                prompt,
                prompt_file,
                ruskel,
                ctx,
                url,
            } => {
                let mut session = tx.load_session()?;

                for f in files.clone().unwrap_or_default() {
                    session.add_editable(&config, &f)?;
                }
                tx.add_contexts(&mut session, ctx, ruskel, url, false, &Some(sender.clone()))
                    .await?;

                let user_prompt = match get_prompt(prompt, prompt_file, &session, false)? {
                    Some(p) => p,
                    None => return Ok(()),
                };
                tx.code(&mut session, user_prompt, Some(sender)).await?;
                Ok(())
            }
            Commands::Session {
                session_file,
                raw,
                render,
                full,
            } => {
                let model = config.active_model()?;
                let session = if let Some(path) = session_file {
                    libtenx::session_store::load_session(path)?
                } else {
                    tx.load_session()?
                };
                if *raw {
                    println!("{:#?}", session);
                } else if *render {
                    println!("{}", model.render(&config, &session)?);
                } else {
                    println!("{}", pretty::session(&config, &session, *full)?);
                }
                Ok(())
            }
            Commands::Edit { files } => {
                let mut session = tx.load_session()?;
                let mut total = 0;

                for file in files {
                    let added = session.add_editable(&config, file)?;
                    if added == 0 {
                        return Err(anyhow::anyhow!("glob did not match any files"));
                    }
                    total += added;
                }

                println!("{} files added for editing", total);
                tx.save_session(&session)?;
                Ok(())
            }
            Commands::Context {
                ruskel,
                file,
                url,
                items,
            } => {
                let mut session = tx.load_session()?;
                let added = tx
                    .add_contexts(
                        &mut session,
                        if *file { items } else { &[] },
                        if *ruskel { items } else { &[] },
                        if *url { items } else { &[] },
                        false,
                        &Some(sender.clone()),
                    )
                    .await?;
                println!("{} context items added", added);
                tx.save_session(&session)?;
                Ok(())
            }
            Commands::Reset { step_offset } => {
                let mut session = tx.load_session()?;
                tx.reset(&mut session, *step_offset)?;
                println!("Session reset to step {}", step_offset);
                Ok(())
            }
            Commands::Retry {
                step_offset,
                edit,
                ctx,
                ruskel,
                url,
                prompt,
                prompt_file,
            } => {
                let mut session = tx.load_session()?;

                let offset = step_offset.unwrap_or(session.steps().len() - 1);
                tx.reset(&mut session, offset)?;

                tx.add_contexts(&mut session, ctx, ruskel, url, false, &Some(sender.clone()))
                    .await?;

                let prompt = if *edit || prompt.is_some() || prompt_file.is_some() {
                    get_prompt(prompt, prompt_file, &session, true)?
                } else {
                    None
                };

                tx.retry(&mut session, prompt, Some(sender.clone())).await?;
                Ok(())
            }
            Commands::New { files, ruskel, url } => {
                let mut session = tx.new_session_from_cwd(&Some(sender.clone())).await?;
                tx.add_contexts(
                    &mut session,
                    files,
                    ruskel,
                    url,
                    false,
                    &Some(sender.clone()),
                )
                .await?;
                tx.save_session(&session)?;
                println!("new session: {}", config.project_root().display());
                Ok(())
            }
            Commands::Fix {
                files,
                ruskel,
                ctx,
                url,
                clear,
                prompt,
                prompt_file,
                edit,
            } => {
                let mut session = if *clear {
                    let mut current_session = tx.load_session()?;
                    current_session.clear();
                    current_session
                } else {
                    tx.new_session_from_cwd(&Some(sender.clone())).await?
                };

                for file in files {
                    session.add_editable(&config, file)?;
                }
                tx.add_contexts(&mut session, ctx, ruskel, url, false, &Some(sender.clone()))
                    .await?;

                let prompt = if prompt.is_some() || prompt_file.is_some() || *edit {
                    get_prompt(prompt, prompt_file, &session, false)?
                } else {
                    None
                };
                tx.fix(&mut session, Some(sender.clone()), prompt).await?;
                Ok(())
            }
            Commands::Clear => {
                let mut session = tx.load_session()?;
                session.clear();
                tx.save_session(&session)?;
                println!("Session cleared");
                Ok(())
            }
            Commands::Check => {
                let mut session = tx.load_session()?;
                tx.run_pre_checks(&mut session, &Some(sender.clone()))?;
                Ok(())
            }
            Commands::Refresh => {
                let mut session = tx.load_session()?;
                tx.refresh_context(&mut session, &Some(sender.clone()))
                    .await?;
                tx.save_session(&session)?;
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

    // Wait for the event task to finish
    let _ = event_kill_tx.send(()).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), event_task).await;

    result?;

    Ok(())
}
