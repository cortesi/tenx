//! A library for building AI-assisted coding tools, with session management, patch validation,
//! and multiple model support.

mod checks;
mod error;
mod tenx;
mod testutils;

pub mod config;
pub mod context;
pub mod dialect;
pub mod event_consumers;
pub mod events;
pub mod model;
pub mod patch;
pub mod pretty;
pub mod session;
pub mod session_store;

pub use error::{Result, TenxError};
pub use tenx::*;
