use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// A snippet of output text received from a model
    Snippet(String),
    /// The preflight check suite has started
    PreflightStart,
    /// The preflight check suite has ended
    PreflightEnd,
    /// A preflight check has passed
    PreflightOk(String),
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
    /// A validation check has passed
    ValidateOk(String),
}
