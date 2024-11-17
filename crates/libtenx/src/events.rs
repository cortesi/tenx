use heck::ToSnakeCase;
use serde::{Deserialize, Serialize};
use serde_variant::to_variant_name;
use tokio::sync::mpsc;

use crate::{Result, TenxError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Helper function to send an event and handle potential errors.
pub fn send_event(sender: &Option<mpsc::Sender<Event>>, event: Event) -> Result<()> {
    if let Some(sender) = sender {
        sender
            .try_send(event)
            .map_err(|e| TenxError::EventSend(e.to_string()))?;
    }
    Ok(())
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

    /// A model request has started
    PromptStart(String),
    /// A model request has completed
    PromptEnd(String),

    /// A snippet of output text received from a model
    Snippet(String),
    /// A a complete, non-streamed response was received from a model
    ModelResponse(String),
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

    /// The command has started
    Start,
    /// The command has finished successfully
    Finish,

    /// A log message with a specified log level
    Log(LogLevel, String),
    /// A retryable error has occurred
    Retry {
        /// An error to display to the user
        user: String,
        /// An error to the model, often the full tool output
        model: String,
    },
    /// A fatal error has occurred
    Fatal(String),
}

impl Event {
    /// Returns the camelcase name of the event variant
    pub fn name(&self) -> String {
        to_variant_name(self).unwrap().to_snake_case()
    }

    /// If this event should have a progress bar or spinner, return an indicator string
    pub fn progress_event(&self) -> Option<String> {
        match self {
            Event::ContextRefreshStart(s) => Some(s.clone()),
            Event::ValidatorStart(s) => Some(s.clone()),
            Event::FormatterStart(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// If this event is a section header, return a string description
    pub fn header_message(&self) -> Option<String> {
        match self {
            Event::ApplyPatch => Some("applying patch".to_string()),
            Event::ContextStart => Some("context".to_string()),
            Event::FormattingStart => Some("formatting".to_string()),
            Event::PreflightStart => Some("preflight validation".to_string()),
            Event::PostPatchStart => Some("post-patch validation".to_string()),
            Event::PromptStart(n) => Some(format!("prompting {}", n)),
            _ => None,
        }
    }

    /// Returns the enclosed string if any, otherwise an empty string
    pub fn display(&self) -> String {
        match self {
            Event::Snippet(s)
            | Event::FormatterStart(s)
            | Event::FormatterEnd(s)
            | Event::ValidatorStart(s) => s.clone(),
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
            Event::PromptStart(model) => Some(format!("Prompting {}...", model)),
            Event::ApplyPatch => Some("Applying patch...".to_string()),
            _ => None,
        }
    }
}

/// Helper struct to manage event sequencing
pub struct EventBlock {
    sender: Option<mpsc::Sender<Event>>,
    end_event: Event,
}

impl EventBlock {
    /// Creates a new EventBlock, emitting the start event immediately
    pub fn new(
        sender: &Option<mpsc::Sender<Event>>,
        start_event: Event,
        end_event: Event,
    ) -> Result<Self> {
        send_event(sender, start_event)?;
        Ok(Self {
            sender: sender.clone(),
            end_event,
        })
    }

    /// Creates a new EventBlock for start/finish operations
    pub fn start(sender: &Option<mpsc::Sender<Event>>) -> Result<Self> {
        Self::new(sender, Event::Start, Event::Finish)
    }

    /// Creates a new EventBlock for context operations
    pub fn context(sender: &Option<mpsc::Sender<Event>>) -> Result<Self> {
        Self::new(sender, Event::ContextStart, Event::ContextEnd)
    }

    /// Creates a new EventBlock for context refresh operations
    pub fn context_refresh(sender: &Option<mpsc::Sender<Event>>, name: &str) -> Result<Self> {
        Self::new(
            sender,
            Event::ContextRefreshStart(name.to_string()),
            Event::ContextRefreshEnd(name.to_string()),
        )
    }

    /// Creates a new EventBlock for formatting operations
    pub fn format(sender: &Option<mpsc::Sender<Event>>) -> Result<Self> {
        Self::new(sender, Event::FormattingStart, Event::FormattingEnd)
    }

    /// Creates a new EventBlock for preflight validation operations
    pub fn preflight(sender: &Option<mpsc::Sender<Event>>) -> Result<Self> {
        Self::new(sender, Event::PreflightStart, Event::PreflightEnd)
    }

    /// Creates a new EventBlock for validator operations
    pub fn validator(sender: &Option<mpsc::Sender<Event>>, name: &str) -> Result<Self> {
        Self::new(
            sender,
            Event::ValidatorStart(name.to_string()),
            Event::ValidatorOk(name.to_string()),
        )
    }

    /// Creates a new EventBlock for post-patch validation operations
    pub fn post_patch(sender: &Option<mpsc::Sender<Event>>) -> Result<Self> {
        Self::new(sender, Event::PostPatchStart, Event::PostPatchEnd)
    }

    /// Creates a new EventBlock for formatter operations
    pub fn formatter(sender: &Option<mpsc::Sender<Event>>, name: &str) -> Result<Self> {
        Self::new(
            sender,
            Event::FormatterStart(name.to_string()),
            Event::FormatterEnd(name.to_string()),
        )
    }

    /// Creates a new EventBlock for model request operations
    pub fn prompt(sender: &Option<mpsc::Sender<Event>>, model: &str) -> Result<Self> {
        Self::new(
            sender,
            Event::PromptStart(model.to_string()),
            Event::PromptEnd(model.to_string()),
        )
    }
}

impl Drop for EventBlock {
    fn drop(&mut self) {
        let _ = send_event(&self.sender, self.end_event.clone());
    }
}
