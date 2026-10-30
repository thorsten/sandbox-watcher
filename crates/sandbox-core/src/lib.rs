//! Core library for Sandbox Watcher.
//!
//! Everything the app knows about MaibornWolff Sandbox containers lives here:
//! the domain model, the `RuntimeAdapter` contract that every container
//! runtime implements, sandbox's on-disk and in-container conventions, and a
//! mock adapter for tests and demos.

pub mod adapter;
pub mod detect;
pub mod docker_compat;
pub mod error;
pub mod mock;
pub mod model;
pub mod overview;
pub mod sandbox;

pub use adapter::RuntimeAdapter;
pub use error::{Error, Result};
