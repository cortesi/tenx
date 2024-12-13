use std::{fs, io::Read, path::PathBuf};

use anyhow::{anyhow, Context as AnyhowContext, Result};
use clap::{CommandFactory, Parser, Subcommand};
use colored::*;
use tokio::sync::mpsc;
use tracing_subscriber::util::SubscriberInitExt;

use libtenx::{
    self,
    config::{self},
    context::Context,
    dialect::DialectProvider,
    event_consumers,
    events::Event,
    model::ModelProvider,
    pretty,
    session::Session,
    Tenx,
};

mod edit;

fn add_files_to_session(
    session: &mut Session,
    config: &config::Config,
    files: &[String],
) -> Result<usize> {
    let mut total = 0;
    for file in files {
        let added = session.add_editable(config, file)?;
        if added == 0 {
            return Err(anyhow!("glob did not match any files: {}", file));
        }
        total += added;
    }
    Ok(total)
}

fn get_prompt(
    prompt: &Option<String>,
    prompt_file: &Option<PathBuf>,
    session: &Session,
    retry: bool,
    event_sender: &Option<mpsc::Sender<Event>>,
) -> Result<Option<String>> {
    if let Some(p) = prompt {
        Ok(Some(p.clone()))
    } else if let Some(file_path) = prompt_file {
        let prompt_content = fs::read_to_string(file_path).context("Failed to read prompt file")?;
        Ok(Some(prompt_content))
    } else {
        Ok(edit::edit_prompt(session, retry, event_sender)?)
    }
}

#[derive(Parser)]
#[clap(name = "tenx")]
#[clap(author = "Aldo Cortesi")]
#[clap(version = env!("CARGO_PKG_VERSION"))]
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
    #[clap(long, env = "TENX_MODEL")]
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
    #[clap(long, conflicts_with = "no_color", env = "TENX_COLOR")]
    color: bool,

    /// Disable colored output
    #[clap(long)]
    no_color: bool,

    // FIXME: Disable these for now
    // /// Smart mode for the Tags dialect
    // #[clap(long)]
    // tags_smart: Option<bool>,
    //
    // /// Replace mode for the Tags dialect
    // #[clap(long)]
    // tags_replace: Option<bool>,
    //
    // /// Udiff mode for the Tags dialect
    // #[clap(long)]
    // tags_udiff: Option<bool>,
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
enum ContextCommands {
    /// Clear all context from the session
    Clear,
    /// Add ruskel documentation to context
    Ruskel {
        /// Items to add to context
        items: Vec<String>,
    },
    /// Add files to context
    File {
        /// Items to add to context
        items: Vec<String>,
    },
    /// Add URLs to context
    Url {
        /// Items to add to context
        items: Vec<String>,
    },
    /// Add text to context
    Text {
        /// Optional name for the text context
        #[clap(long)]
        name: Option<String>,
        /// File to read text from (reads from stdin if not specified)
        file: Option<String>,
    },
    /// Show the current session's contexts
    Show,
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
    Check {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Option<Vec<String>>,
    },
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
    },
    /// Print the current configuration
    #[clap(alias = "config")]
    Conf {
        /// Output default configuration
        #[clap(long)]
        defaults: bool,
    },
    /// Add items to context (alias: ctx)
    #[clap(alias = "ctx")]
    Context {
        #[clap(subcommand)]
        command: ContextCommands,
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
        /// Clear the current session, and use it to fix
        #[clap(long)]
        clear: bool,
        /// Skip adding default context to new session
        #[clap(long)]
        no_ctx: bool,
        /// User prompt for the fix operation
        #[clap(long)]
        prompt: Option<String>,
        /// Path to a file containing the prompt
        #[clap(long)]
        prompt_file: Option<PathBuf>,
        /// Edit the prompt before fixing
        #[clap(long)]
        edit: bool,
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Option<Vec<String>>,
    },
    /// List configured models
    Models {
        /// Show full configuration details
        #[clap(short, long)]
        full: bool,
    },
    /// Create a new session
    New {
        /// Skip adding default context to new session
        #[clap(long)]
        no_ctx: bool,
    },
    /// Print information about the current project
    Project,
    /// Start a new session, edit the prompt, and run it
    Quick {
        /// Specifies files to edit
        #[clap(value_parser)]
        files: Vec<String>,
        /// Skip adding default context to new session
        #[clap(long)]
        no_ctx: bool,
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
    /// Retry a prompt
    Retry {
        /// The step offset to retry from
        step_offset: Option<usize>,
        /// Edit the prompt before retrying
        #[clap(long)]
        edit: bool,
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
    let current_dir = std::env::current_dir()?;
    let mut config = config::load_config(&current_dir)?;

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
    // set_config!(config, tags.smart, cli.tags_smart);
    // set_config!(config, tags.replace, cli.tags_replace);
    // set_config!(config, tags.udiff, cli.tags_udiff);
    if let Some(model) = &cli.model {
        config.models.default = model.clone();
    }
    config.checks.no_pre = cli.no_pre_check;
    config.checks.only = cli.only_check.clone();
    config.models.no_stream = cli.no_stream;

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
    sigpipe::reset();
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
                for model in &config.model_confs() {
                    match model {
                        libtenx::config::Model::Claude { .. }
                        | libtenx::config::Model::OpenAi { .. } => {
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
            Commands::Conf { defaults } => {
                let conf = if *defaults {
                    config::default_config(std::env::current_dir()?)
                } else {
                    config.clone()
                };
                println!("{}", conf.to_ron()?);
                Ok(()) as anyhow::Result<()>
            }
            Commands::Project => {
                print!("{}", pretty::print_project(&config));
                Ok(())
            }
            Commands::Files { pattern } => {
                let files = if let Some(p) = pattern {
                    config.match_files_with_glob(p)?
                } else {
                    config.project_files()?
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
                no_ctx,
                prompt,
                prompt_file,
            } => {
                let mut session = tx
                    .new_session_from_cwd(&Some(sender.clone()), *no_ctx)
                    .await?;
                add_files_to_session(&mut session, &config, files)?;
                let user_prompt = match get_prompt(
                    prompt,
                    prompt_file,
                    &session,
                    false,
                    &Some(sender.clone()),
                )? {
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
            } => {
                let mut session = match tx.load_session() {
                    Ok(sess) => sess,
                    Err(_) => {
                        println!("No existing session to check.");
                        return Ok(());
                    }
                };
                if let Some(files) = files {
                    add_files_to_session(&mut session, &config, files)?;
                }
                let user_prompt = match get_prompt(
                    prompt,
                    prompt_file,
                    &session,
                    false,
                    &Some(sender.clone()),
                )? {
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
                    println!("{}", pretty::print_session(&config, &session, *full)?);
                }
                Ok(())
            }
            Commands::Edit { files } => {
                let mut session = tx.load_session()?;
                let total = add_files_to_session(&mut session, &config, files)?;
                println!("{} files added for editing", total);
                tx.save_session(&session)?;
                Ok(())
            }
            Commands::Context { command } => {
                let mut session = tx.load_session()?;
                match command {
                    ContextCommands::Clear => {
                        session.clear_ctx();
                        println!("All context cleared from session");
                    }
                    ContextCommands::Ruskel { items } => {
                        for item in items {
                            session.add_context(Context::new_ruskel(item));
                        }
                    }
                    ContextCommands::File { items } => {
                        for item in items {
                            session.add_context(Context::new_path(&config, item)?);
                        }
                    }
                    ContextCommands::Url { items } => {
                        for item in items {
                            session.add_context(Context::new_url(item));
                        }
                    }
                    ContextCommands::Text { name, file } => {
                        let text = if let Some(path) = file {
                            fs::read_to_string(path).context("Failed to read text file")?
                        } else {
                            let mut buffer = String::new();
                            std::io::stdin().read_to_string(&mut buffer)?;
                            buffer
                        };
                        let name = name.as_deref().unwrap_or("<anonymous>");
                        session.add_context(Context::new_text(name, &text));
                    }
                    ContextCommands::Show => {
                        if session.contexts().is_empty() {
                            println!("No contexts in session");
                            return Ok(());
                        }
                        println!("{}", pretty::print_contexts(&config, &session)?);
                        return Ok(());
                    }
                };
                tx.refresh_needed_contexts(&mut session, &Some(sender.clone()))
                    .await?;
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
                prompt,
                prompt_file,
            } => {
                let mut session = tx.load_session()?;

                let offset = step_offset.unwrap_or(session.steps().len() - 1);
                tx.reset(&mut session, offset)?;

                let prompt = if *edit || prompt.is_some() || prompt_file.is_some() {
                    get_prompt(prompt, prompt_file, &session, true, &Some(sender.clone()))?
                } else {
                    None
                };

                tx.retry(&mut session, prompt, Some(sender.clone())).await?;
                Ok(())
            }
            Commands::New { no_ctx } => {
                let session = tx
                    .new_session_from_cwd(&Some(sender.clone()), *no_ctx)
                    .await?;
                tx.save_session(&session)?;

                println!("{}", pretty::print_session(&config, &session, false)?);
                Ok(())
            }
            Commands::Fix {
                clear,
                no_ctx,
                prompt,
                prompt_file,
                edit,
                files,
            } => {
                let mut session = if *clear {
                    let mut current_session = tx.load_session()?;
                    current_session.clear();
                    current_session
                } else {
                    tx.new_session_from_cwd(&Some(sender.clone()), *no_ctx)
                        .await?
                };

                if let Some(files) = files {
                    add_files_to_session(&mut session, &config, files)?;
                }

                let prompt = if prompt.is_some() || prompt_file.is_some() || *edit {
                    get_prompt(prompt, prompt_file, &session, false, &Some(sender.clone()))?
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
            Commands::Check { files } => {
                let mut session = tx.load_session()?;
                if let Some(files) = files {
                    add_files_to_session(&mut session, &config, files)?;
                }
                match tx.check(&mut session, &Some(sender.clone())) {
                    Ok(_) => Ok(()),
                    Err(e) => match e {
                        libtenx::TenxError::Check { name, user, model } => Err(anyhow!(
                            "Check '{}' failed: {}\nfull output:\n{}",
                            name,
                            user,
                            model
                        )),
                        other => Err(other.into()),
                    },
                }
            }
            Commands::Refresh => {
                let mut session = tx.load_session()?;
                tx.refresh_contexts(&mut session, &Some(sender.clone()))
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
