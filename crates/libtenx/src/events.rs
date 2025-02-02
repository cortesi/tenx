//! Events emitted by Tenx during operation, for display to users.
use heck::ToSnakeCase;
use serde::{Deserialize, Serialize};
use serde_variant::to_variant_name;
use tokio::sync::mpsc;

use crate::{Result, TenxError};

pub type EventSender = mpsc::Sender<Event>;
pub type EventReceiver = mpsc::Receiver<Event>;

/// Log levels used in events to indicate severity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Helper function to send an event and handle potential errors.
pub fn send_event(sender: &Option<EventSender>, event: Event) -> Result<()> {
    if let Some(sender) = sender {
        sender
            .try_send(event)
            .map_err(|e| TenxError::EventSend(e.to_string()))?;
    }
    Ok(())
}

// The events are listed below roughly in the order they are expected to occur

/// Events emitted during execution to track progress and provide feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// The pre check suite has started
    PreCheckStart,
    /// The pre check suite has ended
    PreCheckEnd,

    /// The post-patch validation suite has started
    PostCheckStart,
    /// The post-patch validation suite has ended
    PostCheckEnd,

    /// Context operations have started
    ContextStart,
    /// Context operations have ended
    ContextEnd,

    /// A context refresh operation started
    ContextRefreshStart(String),
    /// A context refresh operation ended
    ContextRefreshEnd(String),

    /// A check has started
    CheckStart(String),
    /// A check has passed
    CheckOk(String),

    /// A model request has started
    PromptStart(String),
    /// A model request has completed
    PromptEnd(String),
    /// We've been throttled for a given number of milliseconds
    Throttled(u64),

    /// A snippet of output text received from a model
    Snippet(String),
    /// A a complete, non-streamed response was received from a model
    ModelResponse(String),
    /// Patch application has started
    ApplyPatch,

    /// The command has started
    Start,

    /// The command has finished successfully
    Finish,

    /// Notify the output subsystem that user interaction is a bout to start.
    /// This is needed in some cases to, for instance, stop a spinner.
    Interact,

    /// A log message with a specified log level
    Log(LogLevel, String),

    /// A retryable error has occurred
    NextStep {
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
            Event::CheckStart(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// If this event is a section header, return a string description
    pub fn header_message(&self) -> Option<String> {
        match self {
            Event::ApplyPatch => Some("applying patch".to_string()),
            Event::ContextStart => Some("preparing context".to_string()),
            Event::PreCheckStart => Some("pre checks".to_string()),
            Event::PostCheckStart => Some("post checks".to_string()),
            Event::PromptStart(n) => Some(format!("prompting {}", n)),
            _ => None,
        }
    }

    /// Returns the enclosed string if any, otherwise an empty string
    pub fn display(&self) -> String {
        match self {
            Event::Snippet(s) | Event::CheckStart(s) => s.clone(),
            Event::Log(_, s) => s.clone(),
            _ => String::new(),
        }
    }

    /// Returns an optional String if there's a commencement message related to the event
    pub fn step_start_message(&self) -> Option<String> {
        match self {
            Event::PreCheckStart => Some("Pre checks...".to_string()),
            Event::PostCheckStart => Some("Post checks...".to_string()),
            Event::CheckStart(name) => Some(format!("Check {}...", name)),
            Event::PromptStart(model) => Some(format!("Prompting {}...", model)),
            Event::ApplyPatch => Some("Applying patch...".to_string()),
            _ => None,
        }
    }
}

/// Helper struct to manage event sequencing
pub struct EventBlock {
    sender: Option<EventSender>,
    end_event: Event,
}

impl EventBlock {
    /// Creates a new EventBlock, emitting the start event immediately
    pub fn new(sender: &Option<EventSender>, start_event: Event, end_event: Event) -> Result<Self> {
        send_event(sender, start_event)?;
        Ok(Self {
            sender: sender.clone(),
            end_event,
        })
    }

    /// Creates a new EventBlock for start/finish operations
    pub fn start(sender: &Option<EventSender>) -> Result<Self> {
        Self::new(sender, Event::Start, Event::Finish)
    }

    /// Creates a new EventBlock for context operations
    pub fn context(sender: &Option<EventSender>) -> Result<Self> {
        Self::new(sender, Event::ContextStart, Event::ContextEnd)
    }

    /// Creates a new EventBlock for context refresh operations
    pub fn context_refresh(sender: &Option<EventSender>, name: &str) -> Result<Self> {
        Self::new(
            sender,
            Event::ContextRefreshStart(name.to_string()),
            Event::ContextRefreshEnd(name.to_string()),
        )
    }

    /// Creates a new EventBlock for pre check operations
    pub fn pre_check(sender: &Option<EventSender>) -> Result<Self> {
        Self::new(sender, Event::PreCheckStart, Event::PreCheckEnd)
    }

    /// Creates a new EventBlock for validator operations
    pub fn check(sender: &Option<EventSender>, name: &str) -> Result<Self> {
        Self::new(
            sender,
            Event::CheckStart(name.to_string()),
            Event::CheckOk(name.to_string()),
        )
    }

    /// Creates a new EventBlock for post-patch validation operations
    pub fn post_check(sender: &Option<EventSender>) -> Result<Self> {
        Self::new(sender, Event::PostCheckStart, Event::PostCheckEnd)
    }

    /// Creates a new EventBlock for model request operations
    pub fn prompt(sender: &Option<EventSender>, model: &str) -> Result<Self> {
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
