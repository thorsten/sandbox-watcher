use thiserror::Error;

/// Errors surfaced by runtime adapters and core operations.
#[derive(Debug, Error)]
pub enum Error {
    /// The runtime binary or socket was not found on this machine.
    #[error("no container runtime detected")]
    RuntimeNotDetected,

    /// The runtime exists but refused or dropped the connection.
    #[error("cannot connect to {runtime}: {reason}")]
    RuntimeUnavailable { runtime: String, reason: String },

    /// The current user may not talk to the runtime socket.
    #[error("permission denied accessing {path}")]
    PermissionDenied { path: String },

    /// The requested container does not exist any more.
    #[error("container {0} not found")]
    ContainerNotFound(String),

    /// The adapter does not implement this operation for its runtime.
    #[error("{operation} is not supported by {runtime}")]
    Unsupported { runtime: String, operation: String },

    /// A response could not be mapped onto the domain model.
    #[error("unexpected runtime response: {0}")]
    Protocol(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
