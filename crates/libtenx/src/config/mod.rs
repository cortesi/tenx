//! Tenx configuration structure, plus serialization and deserialization from the standard config
//! format.
#[allow(clippy::module_inception)]
mod config;
mod defaults;

pub use config::{
    load_config, CheckConfig, Checks, Config, ConfigFile, Context, Dialect, Include, Model, Models,
    Project, Root, Tags, TextContext,
};

pub use defaults::*;
