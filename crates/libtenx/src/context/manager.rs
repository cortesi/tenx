use super::{Context, ContextProvider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::iter::IntoIterator;

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

    /// Get all contexts as a vector.
    pub fn as_vec(&self) -> Vec<Context> {
        self.contexts.values().cloned().collect()
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

    /// Check if a context with the same ID exists in the manager.
    pub fn contains(&self, context: &Context) -> bool {
        self.contexts.contains_key(&context.id())
    }

    /// Find position of a context in the manager's values.
    pub fn position<F>(&self, predicate: F) -> Option<usize>
    where
        F: FnMut(&&Context) -> bool,
    {
        self.list().iter().position(predicate)
    }

    /// Creates a new ContextManager from a vector of contexts.
    pub fn from_vec(contexts: Vec<Context>) -> Self {
        let mut manager = Self::new();
        for context in contexts {
            manager.add(context);
        }
        manager
    }

    /// Returns a mutable iterator over the contexts.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Context> {
        self.contexts.values_mut()
    }

    /// Pushes a context to the manager.
    pub fn push(&mut self, context: Context) {
        self.add(context);
    }
}

impl<'a> IntoIterator for &'a ContextManager {
    type Item = &'a Context;
    type IntoIter = std::collections::hash_map::Values<'a, String, Context>;

    fn into_iter(self) -> Self::IntoIter {
        self.contexts.values()
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

        // Test as_vec() method
        let contexts_vec = manager.as_vec();
        assert_eq!(contexts_vec.len(), 2);

        // Test from_vec() method
        let new_manager = ContextManager::from_vec(contexts_vec);
        assert_eq!(new_manager.len(), 2);

        // Clear all contexts
        manager.clear();
        assert!(manager.is_empty());
    }
}
