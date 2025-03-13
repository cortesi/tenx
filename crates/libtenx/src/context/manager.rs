use super::{Context, ContextProvider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A manager for a collection of context items.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ContextManager {
    /// A map of context items with their IDs as keys.
    contexts: HashMap<String, Context>,
}

impl ContextManager {
    /// Creates a new empty ContextManager.
    pub fn new() -> Self {
        Self {
            contexts: HashMap::new(),
        }
    }

    /// Adds a context item to the manager.
    /// If a duplicate context already exists, it will be replaced.
    pub fn add(&mut self, context: Context) {
        let id = context.id();
        self.contexts.insert(id, context);
    }

    /// Returns a list of all contexts.
    pub fn list(&self) -> Vec<&Context> {
        self.contexts.values().collect()
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

        let human_strings: Vec<String> = manager.list().iter().map(|c| c.human()).collect();
        assert!(human_strings.contains(&"text: test1 (1 lines, 8 chars)".to_string()));

        // Add another context
        let context2 = Context::new_text("test2", "content2");
        manager.add(context2);
        assert_eq!(manager.len(), 2);

        // Add a duplicate context (should replace the first one)
        let context3 = Context::new_text("test1", "updated content");
        manager.add(context3);
        assert_eq!(manager.len(), 2);

        let human_strings: Vec<String> = manager.list().iter().map(|c| c.human()).collect();
        assert!(human_strings.contains(&"text: test1 (1 lines, 15 chars)".to_string()));

        // Clear all contexts
        manager.clear();
        assert!(manager.is_empty());
    }
}
