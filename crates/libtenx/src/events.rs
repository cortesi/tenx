use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    Snippet(String),
    PreflightStart,
    PreflightEnd,
    ValidationStart,
    ValidationEnd,
}
