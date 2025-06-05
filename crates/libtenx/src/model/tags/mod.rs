#[allow(clippy::module_inception)]
mod tags;

#[cfg(test)]
mod tags_test;

mod xmlish;

pub use tags::*;
