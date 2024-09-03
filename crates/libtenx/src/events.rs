use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

// The events are listed below roughly in the order they are expected to occur

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// The preflight check suite has started
    PreflightStart,
    /// The preflight check suite has ended
    PreflightEnd,

    /// A model prompt has started
    PromptStart,
    /// A snippet of output text received from a model
    Snippet(String),
    /// Prompt has completed successfully
    PromptDone,
    /// Patch application has started
    ApplyPatch,

    /// The formatting suite has started
    FormattingStart,
    /// The formatting suite has ended
    FormattingEnd,
    /// A formatter has run successfully
    FormattingOk(String),

    /// The validation suite has started
    ValidationStart,
    /// A a preflight or post-patch check has started
    ValidatorStart(String),
    /// A a preflight or post-patch check has passed
    ValidatorOk(String),
    /// The validation suite has ended
    ValidationEnd,

    /// The session has finished successfully
    Finish,

    /// A log message with a specified log level
    Log(LogLevel, String),
    /// A retryable error has occurred
    Retry(String),
    /// A fatal error has occurred
    Fatal(String),
}

impl Event {
    /// Returns the camelcase name of the event variant
    pub fn name(&self) -> &'static str {
        match self {
            Event::PreflightStart => "preflight_start",
            Event::PreflightEnd => "preflight_end",

            Event::PromptStart => "prompt_start",
            Event::Snippet(_) => "snippet",
            Event::PromptDone => "prompt_done",
            Event::ApplyPatch => "apply_patch",

            Event::FormattingStart => "formatting_start",
            Event::FormattingEnd => "formatting_end",
            Event::FormattingOk(_) => "formatting_ok",

            Event::ValidationStart => "validation_start",
            Event::ValidatorStart(_) => "check_start",
            Event::ValidatorOk(_) => "check_ok",
            Event::ValidationEnd => "validation_end",

            Event::Log(_, _) => "log",
            Event::Retry(_) => "retry",
            Event::Fatal(_) => "fatal",
            Event::Finish => "finish",
        }
    }

    /// Returns the enclosed string if any, otherwise an empty string
    pub fn display(&self) -> String {
        match self {
            Event::Snippet(s)
            | Event::FormattingOk(s)
            | Event::ValidatorStart(s)
            | Event::ValidatorOk(s) => s.clone(),
            Event::Log(_, s) => s.clone(),
            _ => String::new(),
        }
    }

    /// Returns an optional String if there's a commencement message related to the event
    pub fn step_start_message(&self) -> Option<String> {
        match self {
            Event::PreflightStart => Some("Preflight checks...".to_string()),
            Event::FormattingStart => Some("Formatting...".to_string()),
            Event::ValidationStart => Some("Post-patch validation...".to_string()),
            Event::ValidatorStart(name) => Some(format!("Validator {}...", name)),
            Event::PromptStart => Some("Prompting...".to_string()),
            Event::ApplyPatch => Some("Applying patch...".to_string()),
            _ => None,
        }
    }
}
