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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Context, ContextProvider};

    #[test]
    fn test_context_manager() {
        let mut manager = ContextManager::new();
        assert!(manager.is_empty());

        // Add a context
        let context1 = Context::new_text("test1", "content1");
        manager.add(context1.clone());
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.list()[0].human(), "text: test1 (1 lines, 8 chars)");

        // Add another context
        let context2 = Context::new_text("test2", "content2");
        manager.add(context2);
        assert_eq!(manager.len(), 2);

        // Add a duplicate context (should replace the first one)
        let context3 = Context::new_text("test1", "updated content");
        manager.add(context3);
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.list()[0].human(), "text: test1 (1 lines, 15 chars)");

        // Clear all contexts
        manager.clear();
        assert!(manager.is_empty());
    }
}