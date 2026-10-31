//! Core library for Sandbox Watcher.
//!
//! Everything the app knows about MaibornWolff Sandbox containers lives here:
//! the domain model, the `RuntimeAdapter` contract that every container
//! runtime implements, sandbox's on-disk and in-container conventions, and a
//! mock adapter for tests and demos.

pub mod actions;
pub mod activity;
pub mod adapter;
pub mod allowlist;
pub mod apple;
pub mod config;
pub mod detect;
pub mod docker;
pub mod docker_compat;
pub mod editor;
pub mod endpoints;
pub mod error;
pub mod files;
pub mod history;
pub mod live;
pub mod mock;
pub mod model;
pub mod mounts;
pub mod network;
pub mod notify;
pub mod overview;
pub mod paths;
pub mod sandbox;
pub mod sessions;
pub mod settings;
pub mod shell_env;
pub mod stats_hub;
pub mod turns;
pub mod usage;

pub use adapter::RuntimeAdapter;
pub use error::{Error, Result};
