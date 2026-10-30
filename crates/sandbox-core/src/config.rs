//! Read the parts of sandbox's user configuration the watcher cares about.
//!
//! Location follows sandbox 0.73.1 (`config-paths.ts`): `$XDG_CONFIG_HOME/sandbox/config.toml`,
//! `%APPDATA%\sandbox\config.toml` on Windows, else `~/.config/sandbox/config.toml`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::RuntimeKind;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxUserConfig {
    pub path: Option<PathBuf>,
    /// The `runtime` key, when set and valid.
    pub runtime: Option<RuntimeKind>,
}

#[derive(Deserialize)]
struct RawConfig {
    runtime: Option<String>,
}

/// Resolve the config file path from the given environment values.
pub fn config_path(
    os: &str,
    xdg_config_home: Option<&str>,
    appdata: Option<&str>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let dir = if let Some(xdg) = xdg_config_home.filter(|v| !v.is_empty()) {
        PathBuf::from(xdg)
    } else if os == "windows" {
        PathBuf::from(appdata.filter(|v| !v.is_empty())?)
    } else {
        home?.join(".config")
    };
    Some(dir.join("sandbox").join("config.toml"))
}

/// Parse the `runtime` key. Unknown values are ignored, not errors.
pub fn parse_runtime(toml_text: &str) -> Option<RuntimeKind> {
    let raw: RawConfig = toml::from_str(toml_text).ok()?;
    match raw.runtime?.as_str() {
        "docker" => Some(RuntimeKind::Docker),
        "podman" => Some(RuntimeKind::Podman),
        "apple-container" => Some(RuntimeKind::AppleContainer),
        _ => None,
    }
}

/// Load from the real environment. Missing or unreadable files yield defaults.
pub fn load() -> SandboxUserConfig {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let path = config_path(
        std::env::consts::OS,
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("APPDATA").ok().as_deref(),
        home.as_deref(),
    );
    let runtime = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| parse_runtime(&t));
    SandboxUserConfig { path, runtime }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths_like_sandbox() {
        let home = Path::new("/home/me");
        assert_eq!(
            config_path("linux", None, None, Some(home)),
            Some(PathBuf::from("/home/me/.config/sandbox/config.toml"))
        );
        assert_eq!(
            config_path("macos", Some("/tmp/xdg"), None, Some(home)),
            Some(PathBuf::from("/tmp/xdg/sandbox/config.toml"))
        );
        assert_eq!(
            config_path("windows", None, Some(r"C:\Users\me\AppData\Roaming"), None),
            Some(
                PathBuf::from(r"C:\Users\me\AppData\Roaming")
                    .join("sandbox")
                    .join("config.toml")
            )
        );
        assert_eq!(config_path("windows", None, None, None), None);
    }

    #[test]
    fn parses_runtime_key() {
        assert_eq!(
            parse_runtime("runtime = \"podman\"\n"),
            Some(RuntimeKind::Podman)
        );
        assert_eq!(
            parse_runtime("runtime = \"apple-container\""),
            Some(RuntimeKind::AppleContainer)
        );
        assert_eq!(parse_runtime("env = [\"FOO\"]\n"), None);
        assert_eq!(parse_runtime("runtime = \"lxc\""), None);
        assert_eq!(parse_runtime("not toml ==="), None);
    }
}
