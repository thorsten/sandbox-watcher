//! The error a Tauri command returns to the webview.
//!
//! Commands fail with a `CommandError`, never a `String`. It serialises as an
//! object with a kebab-case `code` and the variant's fields, so the webview
//! can pick the sentence for `code` from its dictionaries and fill the fields
//! in (`src/errors.ts`). What Docker, ssh, the OS or a remote script said
//! travels untranslated in `detail` or `reason`; that text is diagnostic, not
//! ours, and is shown as it is. See `docs/adr/0003-localisation.md`.
//!
//! Adding a variant: a dictionary key `error.<code>` in `en.ts` and `de.ts`,
//! a sample in `tests/contracts.rs`, and the `KEYS` entry in `src/errors.ts`
//! (`tsc` fails without it).

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum CommandError {
    /// A host-side action (open, reveal, terminal) on a remote container.
    LocalOnly,
    /// The first refresh has not produced an overview yet.
    OverviewNotReady,
    /// No runtime adapter knows this container key.
    UnknownContainer { container: String },
    /// The runtime no longer has the container.
    ContainerNotFound { container: String },
    /// The path to reveal is not a bind mount of the container.
    NotAMount,
    /// The container has no bind-mounted project directory.
    NoProjectDirectory,
    /// The file to open lies outside the container's project directory.
    PathOutsideProject,
    /// None of the editor candidates could be started.
    NoEditor { detail: String },
    /// None of the terminal candidates could be started.
    NoTerminal { detail: String },
    /// The system opener refused a path.
    OpenFailed { detail: String },
    /// No global sandbox config path exists on this machine.
    NoGlobalConfig,
    /// The project has no `.sandbox` directory to write a config into.
    NoSandboxDirectory,
    /// A remote write needs the hash from a preview first.
    PreviewRequired,
    /// The remote config changed since the preview; nothing was written.
    ConfigChanged { host: String },
    /// The path rules refused the write (symlink, not a regular file).
    ConfigRefused { path: String, reason: String },
    /// Writing the config failed on this machine.
    ConfigWriteFailed { path: String, detail: String },
    /// The alias does not satisfy `host::validate_alias`.
    InvalidAlias { host: String, reason: String },
    /// The remote host has no open connection right now.
    HostNotConnected { host: String },
    /// No directory to store the settings file in.
    NoConfigDirectory,
    /// Writing the settings file failed.
    SettingsWriteFailed { path: String, detail: String },
    /// The release endpoint could not be asked.
    UpdateCheckFailed { detail: String },
    /// Install was asked without a prior successful check.
    NoUpdatePending,
    /// Download, verification or install of the update failed.
    UpdateInstallFailed { detail: String },
    /// `Error::RuntimeNotDetected`.
    RuntimeNotDetected,
    /// `Error::RuntimeUnavailable`.
    RuntimeUnavailable { runtime: String, reason: String },
    /// `Error::PermissionDenied`.
    PermissionDenied { path: String },
    /// `Error::Unsupported`.
    Unsupported { runtime: String, operation: String },
    /// `Error::Protocol`, `Error::Io`, `Error::Json`: the runtime answered,
    /// but not in a way the watcher could use.
    RuntimeResponse { detail: String },
    /// Anything without a sentence of its own: a remote script that replied
    /// unexpectedly, the mock mode, the native menu. Shown as the detail.
    Other { detail: String },
}

impl CommandError {
    pub fn other(detail: impl Into<String>) -> Self {
        CommandError::Other {
            detail: detail.into(),
        }
    }
}

impl From<Error> for CommandError {
    fn from(e: Error) -> Self {
        match e {
            Error::RuntimeNotDetected => CommandError::RuntimeNotDetected,
            Error::RuntimeUnavailable { runtime, reason } => {
                CommandError::RuntimeUnavailable { runtime, reason }
            }
            Error::PermissionDenied { path } => CommandError::PermissionDenied { path },
            Error::ContainerNotFound(container) => CommandError::ContainerNotFound { container },
            Error::Unsupported { runtime, operation } => {
                CommandError::Unsupported { runtime, operation }
            }
            Error::Protocol(detail) => CommandError::RuntimeResponse { detail },
            Error::Io(e) => CommandError::RuntimeResponse {
                detail: e.to_string(),
            },
            Error::Json(e) => CommandError::RuntimeResponse {
                detail: e.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_with_a_kebab_case_code_and_the_fields() {
        let json = serde_json::to_value(CommandError::ConfigRefused {
            path: "/etc/x".into(),
            reason: "symlink".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "code": "config-refused", "path": "/etc/x", "reason": "symlink" })
        );
        assert_eq!(
            serde_json::to_value(CommandError::LocalOnly).unwrap(),
            serde_json::json!({ "code": "local-only" })
        );
    }

    #[test]
    fn runtime_errors_keep_their_structure() {
        assert_eq!(
            CommandError::from(Error::ContainerNotFound("abc".into())),
            CommandError::ContainerNotFound {
                container: "abc".into()
            }
        );
        assert_eq!(
            CommandError::from(Error::Unsupported {
                runtime: "apple-container".into(),
                operation: "stats".into()
            }),
            CommandError::Unsupported {
                runtime: "apple-container".into(),
                operation: "stats".into()
            }
        );
        let io = Error::Io(std::io::Error::other("socket closed"));
        assert_eq!(
            CommandError::from(io),
            CommandError::RuntimeResponse {
                detail: "socket closed".into()
            }
        );
    }
}
