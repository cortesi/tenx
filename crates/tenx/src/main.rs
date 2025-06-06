use std::{fs, io::Read, path::PathBuf};

use anyhow::{anyhow, Context as AnyhowContext, Result};
use clap::{CommandFactory, Parser, Subcommand};
use colored::*;
use tokio::sync::mpsc;
use tracing_subscriber::util::SubscriberInitExt;

use libtenx::{
    config::{self},
    context::Context,
    event_consumers,
    events::Event,
    session::Session,
    Tenx,
};
use unirend::Detail;

mod edit;

/// Parse a step offset string in format "action" or "action:step" and return the parsed indices
/// If the step is not specified (format "action"), the step index will be None.
fn parse_step_offset(offset_str: &str) -> Result<(usize, Option<usize>)> {
    // Parse the action:step format or just action
    let parts: Vec<&str> = offset_str.split(':').collect();

    if parts.is_empty() || parts.len() > 2 {
        return Err(anyhow!(
            "Step offset must be in format 'action' or 'action:step', e.g. '0' or '0:3'"
        ));
    }

    let action_idx = parts[0]
        .parse::<usize>()
        .map_err(|_| anyhow!("Invalid action index: {}", parts[0]))?;

    let step_idx = if parts.len() == 2 {
        Some(
            parts[1]
                .parse::<usize>()
                .map_err(|_| anyhow!("Invalid step index: {}", parts[1]))?,
        )
    } else {
        None
    };

    Ok((action_idx, step_idx))
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

    /// Limit number of steps after a prompt
    #[clap(long)]
    step_limit: Option<usize>,

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
    /// Refresh all contexts in the current session
    Refresh,
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
    /// Add command output to context
    Cmd {
        /// Command to execute
        command: String,
    },
    /// Show the current session's contexts
    Show,
}

#[derive(Subcommand)]
enum Commands {
    /// Run check suite all project files, or a subet
    Check {
        /// Files to check, glob patterns accepted
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
    /// Continue with the current session
    Continue {
        /// User prompt for the operation
        #[clap(long)]
        prompt: Option<String>,
        /// Path to a file containing the prompt
        #[clap(long)]
        prompt_file: Option<PathBuf>,
    },
    /// Make an AI-assisted change using the current session
    Code {
        /// Specifies files to edit, glob patterns accepted
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
    /// Context commands (alias: ctx)
    #[clap(alias = "ctx")]
    Context {
        #[clap(subcommand)]
        command: ContextCommands,
    },
    /// Add editable files to a session
    Edit {
        /// Specifies files to edit, glob patterns accepted
        #[clap(value_parser, required = true)]
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
        /// Specifies files to edit, glob patterns accepted
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
        /// Specifies files to edit, glob patterns accepted
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
    /// Reset the session to a specific step, undoing changes
    Reset {
        /// The step offset to reset to, in format "action:step" (e.g. "0:3")
        step_offset: Option<String>,
        /// Reset all steps in the session
        #[clap(long)]
        all: bool,
    },
    /// Retry a prompt
    Retry {
        /// The step offset to retry from, in format "action:step" (e.g. "0:3")
        step_offset: Option<String>,
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
        /// Format to display the session in
        #[clap(long, value_parser = ["pretty", "raw", "render"], default_value = "pretty")]
        fmt: String,
        /// Increase detail level (can be used multiple times)
        #[clap(short = 'd', action = clap::ArgAction::Count, default_value = "0")]
        detail: u8,
        /// Show short output (less detail)
        #[clap(short, long, conflicts_with = "detail")]
        short: bool,
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
                }
            }

    // Apply CLI arguments
    config = config.load_env();
    set_config!(config, session_store_dir, cli.session_store_dir.clone());
    set_config!(config, step_limit, cli.step_limit);
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
        Some(cmd) => {
            match cmd {
                Commands::Models { full } => {
                    for model in &config.model_confs() {
                        println!("{}", model.name().blue().bold());
                        println!("    kind: {}", model.kind());
                        for line in model.text_config(*full).lines() {
                            println!("    {line}");
                        }
                        println!();
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
                    // FIXME: Implement this
                    // print!("{}", pretty::print_project(&config));
                    Ok(())
                }
                Commands::Files { pattern } => {
                    let state = config.state()?;
                    let files = if let Some(p) = pattern {
                        state.find(std::env::current_dir()?, vec![p.to_string()])?
                    } else {
                        state.list()?
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
                        println!();
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
                    tx.code(&mut session)?;
                    // Add files to the session
                    if !files.is_empty() {
                        session
                            .last_action_mut()?
                            .state
                            .view(&config.cwd()?, files.to_vec())?;
                    }
                    tx.continue_steps(&mut session, Some(user_prompt), Some(sender.clone()), None)
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
                    tx.code(&mut session)?;

                    // Add files to the action if provided
                    if let Some(file_list) = &files {
                        if !file_list.is_empty() {
                            session
                                .last_action_mut()?
                                .state
                                .view(&config.cwd()?, file_list.to_vec())?;
                        }
                    }

                    tx.continue_steps(&mut session, Some(user_prompt), Some(sender), None)
                        .await?;
                    Ok(())
                }
                Commands::Session {
                    session_file,
                    fmt,
                    detail,
                    short,
                } => {
                    let session = if let Some(path) = session_file {
                        libtenx::session_store::load_session(path)?
                    } else {
                        tx.load_session()?
                    };

                    match fmt.as_str() {
                        "raw" => {
                            println!("{session:#?}");
                        }
                        "render" => {
                            // FIXME: Use chat
                            // println!("{}", model.render(&config, &session)?);
                        }
                        _ => {
                            // Determine detail level
                            let detail_level = if *short {
                                Detail::Short
                            } else {
                                match detail {
                                    0 => Detail::Default,
                                    1 => Detail::Detailed,
                                    _ => Detail::Full,
                                }
                            };

                            // Use the Term renderer to render the session
                            let mut renderer = unirend::Term::new();
                            session.render(&config, &mut renderer, detail_level)?;
                            println!("{}", renderer.render());
                        }
                    }
                    Ok(())
                }
                Commands::Edit { files } => {
                    let mut session = tx.load_session()?;
                    let total = tx.edit(&mut session, files)?;
                    println!("{total} files added for editing");
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
                        ContextCommands::Refresh => {
                            tx.refresh_contexts(&mut session, &Some(sender.clone()))
                                .await?;
                            tx.save_session(&session)?;
                            println!("Contexts refreshed.");
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
                        ContextCommands::Cmd { command } => {
                            session.add_context(Context::new_cmd(command));
                        }
                        ContextCommands::Show => {
                            if session.contexts.is_empty() {
                                println!("No contexts in session");
                            } else {
                                let mut render = unirend::Term::new();
                                session.contexts.render(&mut render, Detail::Default)?;
                                println!("{}", render.render());
                            }
                            return Ok(());
                        }
                    };
                    tx.refresh_needed_contexts(&mut session, &Some(sender.clone()))
                        .await?;
                    tx.save_session(&session)?;
                    Ok(())
                }
                Commands::Reset { step_offset, all } => {
                    if *all && step_offset.is_some() {
                        return Err(anyhow!("Cannot specify both --all and a step offset"));
                    }
                    let mut session = tx.load_session()?;
                    if *all {
                        tx.reset_all(&mut session)?;
                        println!("All steps reset");
                    } else {
                        let offset_str = step_offset
                        .as_ref()
                        .ok_or_else(|| anyhow!("Must specify either --all or a step offset in format 'action:step'"))?;

                        let (action_idx, step_idx) = parse_step_offset(offset_str)?;

                        tx.reset(&mut session, action_idx, step_idx)?;

                        println!("Session reset to step {offset_str}");
                    }
                    Ok(())
                }
                Commands::Retry {
                    step_offset,
                    edit,
                    prompt,
                    prompt_file,
                } => {
                    let mut session = tx.load_session()?;

                    // Parse the step offset if provided
                    let (action_idx, step_idx) = if let Some(offset_str) = step_offset {
                        let (a, s) = parse_step_offset(offset_str)?;
                        (Some(a), s)
                    } else {
                        (None, None)
                    };

                    // Get prompt if needed
                    let prompt = if *edit || prompt.is_some() || prompt_file.is_some() {
                        get_prompt(prompt, prompt_file, &session, true, &Some(sender.clone()))?
                    } else {
                        None
                    };

                    // Retry the step and continue
                    tx.retry(&mut session, action_idx, step_idx)?;
                    tx.continue_steps(&mut session, prompt, Some(sender.clone()), None)
                        .await?;
                    Ok(())
                }
                Commands::New { no_ctx } => {
                    let session = tx
                        .new_session_from_cwd(&Some(sender.clone()), *no_ctx)
                        .await?;
                    tx.save_session(&session)?;

                    let mut renderer = unirend::Term::new();
                    session.render(&config, &mut renderer, Detail::Default)?;
                    println!("{}", renderer.render());

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

                    let user_prompt = if prompt.is_some() || prompt_file.is_some() || *edit {
                        get_prompt(prompt, prompt_file, &session, false, &Some(sender.clone()))?
                    } else {
                        None
                    };
                    tx.fix(&mut session)?;
                    // Add files to the session if provided
                    if let Some(file_list) = &files {
                        if !file_list.is_empty() {
                            session
                                .last_action_mut()?
                                .state
                                .view(&config.cwd()?, file_list.to_vec())?;
                        }
                    }

                    tx.continue_steps(&mut session, user_prompt, Some(sender.clone()), None)
                        .await?;
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
                    let paths = if let Some(files) = files {
                        let mut matched = Vec::new();
                        for pattern in files {
                            let glob_matches = config.match_files_with_glob(pattern)?;
                            matched.extend(glob_matches);
                        }
                        matched
                    } else {
                        config.project_files()?
                    };
                    match tx.check(paths, &Some(sender.clone())) {
                        Ok(_) => Ok(()),
                        Err(e) => match e {
                            other => Err(other.into()),
                        },
                    }
                }
                Commands::Continue {
                    prompt,
                    prompt_file,
                } => {
                    let mut session = match tx.load_session() {
                        Ok(sess) => sess,
                        Err(_) => {
                            println!("No existing session found.");
                            return Ok(());
                        }
                    };

                    let user_prompt =
                        get_prompt(prompt, prompt_file, &session, false, &Some(sender.clone()))?;

                    tx.continue_steps(&mut session, user_prompt, Some(sender.clone()), None)
                        .await?;
                    Ok(())
                }
            }
        }
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
