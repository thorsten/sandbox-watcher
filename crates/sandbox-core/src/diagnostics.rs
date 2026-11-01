//! Types for the diagnostics page: what was tried and how it turned out.

use serde::{Deserialize, Serialize};

use crate::model::RuntimeKind;

/// How one detection candidate turned out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum ProbeOutcome {
    Found { endpoint: String },
    Missing,
    Skipped { reason: String },
    Failed { reason: String },
}

/// One candidate that detection tried, with its outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub kind: RuntimeKind,
    /// The socket path, variable name or `<binary> on PATH`, as shown to the user.
    pub candidate: String,
    #[serde(flatten)]
    pub outcome: ProbeOutcome,
}
