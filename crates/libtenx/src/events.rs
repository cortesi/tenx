use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// A snippet of output text received from a model
    Snippet(String),
    /// The preflight check suite has started
    PreflightStart,
    /// The preflight check suite has ended
    PreflightEnd,
    /// The formatting suite has started
    FormattingStart,
    /// The formatting suite has ended
    FormattingEnd,
    /// A formatter has run successfully
    FormattingOk(String),
    /// The validation suite has started
    ValidationStart,
    /// The validation suite has ended
    ValidationEnd,
    /// A a preflight or post-patch check has started
    CheckStart(String),
    /// A a preflight or post-patch check has passed
    CheckOk(String),
    /// A log message with a specified log level
    Log(LogLevel, String),
    /// A model prompt has started
    PromptStart,
    /// Prompt has completed successfully
    PromptDone,
    /// Patch application has started
    ApplyPatch,
    /// A retryable error has occurred
    Retry(String),
    /// A fatal error has occurred
    Fatal(String),
}

impl Event {
    /// Returns the camelcase name of the event variant
    pub fn name(&self) -> &'static str {
        match self {
            Event::Snippet(_) => "snippet",
            Event::PreflightStart => "preflight_start",
            Event::PreflightEnd => "preflight_end",
            Event::FormattingStart => "formatting_start",
            Event::FormattingEnd => "formatting_end",
            Event::FormattingOk(_) => "formatting_ok",
            Event::ValidationStart => "validation_start",
            Event::ValidationEnd => "validation_end",
            Event::CheckStart(_) => "check_start",
            Event::CheckOk(_) => "check_ok",
            Event::Log(_, _) => "log",
            Event::PromptStart => "prompt_start",
            Event::ApplyPatch => "apply_patch",
            Event::Retry(_) => "retry",
            Event::Fatal(_) => "fatal",
            Event::PromptDone => "prompt_done",
        }
    }

    /// Returns the enclosed string if any, otherwise an empty string
    pub fn display(&self) -> String {
        match self {
            Event::Snippet(s)
            | Event::FormattingOk(s)
            | Event::CheckStart(s)
            | Event::CheckOk(s) => s.clone(),
            Event::Log(_, s) => s.clone(),
            _ => String::new(),
        }
    }

    /// Returns an optional String if there's a commencement message related to the event
    pub fn step_start_message(&self) -> Option<String> {
        match self {
            Event::PreflightStart => Some("Starting preflight checks...".to_string()),
            Event::FormattingStart => Some("Starting formatting...".to_string()),
            Event::ValidationStart => Some("Starting post-patch validation...".to_string()),
            Event::CheckStart(name) => Some(format!("Checking {}...", name)),
            Event::PromptStart => Some("Prompting model...".to_string()),
            Event::ApplyPatch => Some("Applying patch...".to_string()),
            _ => None,
        }
    }
}
