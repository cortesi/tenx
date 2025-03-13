use super::Context;
use serde::{Deserialize, Serialize};

/// A manager for a collection of context items.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ContextManager {
    /// An ordered list of context items.
    contexts: Vec<Context>,
}

impl ContextManager {
    /// Creates a new empty ContextManager.
    pub fn new() -> Self {
        Self {
            contexts: Vec::new(),
        }
    }

    /// Adds a context item to the manager.
    /// If a duplicate context already exists, it will be replaced.
    pub fn add(&mut self, context: Context) {
        // Find any existing duplicates
        if let Some(index) = self.contexts.iter().position(|c| c.is_dupe(&context)) {
            // Replace the duplicate with the new context
            self.contexts[index] = context;
        } else {
            // Add the new context
            self.contexts.push(context);
        }
    }

    /// Returns a list of all contexts.
    pub fn list(&self) -> &[Context] {
        &self.contexts
    }

    /// Clears all contexts.
    pub fn clear(&mut self) {
        self.contexts.clear();
    }

    /// Returns the number of contexts in the manager.
    pub fn len(&self) -> usize {
        self.contexts.len()
    }

    /// Returns true if the manager has no contexts.
    pub fn is_empty(&self) -> bool {
        self.contexts.is_empty()
    }
}
