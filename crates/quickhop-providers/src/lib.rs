//! Built-in quickhop providers. Each module will implement [`quickhop_core::Provider`];
//! the implementations land with task T91.

#![forbid(unsafe_code)]

pub mod apps;
pub mod calc;
pub mod commands;
pub mod files;

/// Names of the built-in providers, matching the keys of `[providers]` in the config.
pub const BUILTIN: [&str; 4] = ["apps", "files", "calc", "commands"];
