mod term;

pub use term::*;

/// A generic trait for rendering output
pub trait Render {
    /// Push a new section onto the stack, with the specified heading
    fn push(&mut self, text: &str);

    /// Pop the current section off the stack
    fn pop(&mut self);

    /// Add a paragraph of text to the current section
    fn para(&mut self, text: &str);

    /// Add a bullet list to the current section
    fn bullets(&mut self, items: Vec<String>);
}