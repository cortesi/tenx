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
    CheckStart(String),
    CheckOk(String),
    /// A log message with a specified log level
    Log(LogLevel, String),
}
