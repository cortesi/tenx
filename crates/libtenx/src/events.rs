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

    /// The post-patch validation suite has started
    PostPatchStart,
    /// The post-patch validation suite has ended
    PostPatchEnd,

    /// Context operations have started
    ContextStart,
    /// Context operations have ended
    ContextEnd,

    /// A context refresh operation started
    ContextRefreshStart(String),
    /// A context refresh operation ended
    ContextRefreshEnd(String),

    /// A a preflight or post-patch check has started
    ValidatorStart(String),
    /// A a preflight or post-patch check has passed
    ValidatorOk(String),

    /// A model prompt has started
    PromptStart,
    /// Prompt has completed successfully
    PromptEnd,

    /// A snippet of output text received from a model
    Snippet(String),
    /// Patch application has started
    ApplyPatch,

    /// The formatting suite has started
    FormattingStart,
    /// A formatter has started running
    FormatterStart(String),
    /// A formatter has finished running
    FormatterEnd(String),
    /// The formatting suite has ended
    FormattingEnd,

    /// The command has finished successfully
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
            Event::ContextStart => "context_start",
            Event::ContextEnd => "context_end",

            Event::ContextRefreshEnd(_) => "context_refresh_end",
            Event::ContextRefreshStart(_) => "context_refresh_start",

            Event::PreflightStart => "preflight_start",
            Event::PreflightEnd => "preflight_end",

            Event::PostPatchStart => "post_patch_start",

            Event::PromptStart => "prompt_start",
            Event::Snippet(_) => "snippet",
            Event::PromptEnd => "prompt_done",
            Event::ApplyPatch => "apply_patch",

            Event::FormattingStart => "formatting_start",
            Event::FormattingEnd => "formatting_end",
            Event::FormatterStart(_) => "formatter_start",
            Event::FormatterEnd(_) => "formatter_end",

            // These events are common for preflight and post-patch validation.
            Event::ValidatorStart(_) => "check_start",
            Event::ValidatorOk(_) => "check_ok",
            Event::PostPatchEnd => "validation_end",

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
            | Event::FormatterStart(s)
            | Event::FormatterEnd(s)
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
            Event::PostPatchStart => Some("Post-patch validation...".to_string()),
            Event::ValidatorStart(name) => Some(format!("Validator {}...", name)),
            Event::PromptStart => Some("Prompting...".to_string()),
            Event::ApplyPatch => Some("Applying patch...".to_string()),
            _ => None,
        }
    }
}
