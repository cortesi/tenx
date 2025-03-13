mod term;

pub use term::*;

/// The amount of detail to include in a render. The the `Render` implementations themselves
/// don't use this - it's here as a common convention for callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    Short,
    Default,
    Detailed,
    Full,
}

pub enum Style {
    H1,
    H2,
    H3,
    H4,
    Warn,
    Error,
    Success,
    Plain,
}

/// A generic trait for rendering output
pub trait Render {
    /// Push a new section onto the stack, with the default heading style
    fn push(&mut self, text: &str);

    /// Push a new section onto the stack, with the specified heading and style
    fn push_style(&mut self, text: &str, style: Style);

    /// Pop the current section off the stack
    fn pop(&mut self);

    /// Add a paragraph of text to the current section
    fn para(&mut self, text: &str);

    /// Add a bullet list to the current section
    fn bullets(&mut self, items: Vec<String>);
}
