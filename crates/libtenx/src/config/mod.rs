//! Tenx configuration structure, plus serialization and deserialization from the standard config
//! format.
#[allow(clippy::module_inception)]
mod config;
mod defaults;
mod files;

pub use config::*;
pub use defaults::*;
