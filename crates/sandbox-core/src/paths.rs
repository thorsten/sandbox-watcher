//! Host path presentation across platforms.

use crate::model::ContainerDetails;

/// Turn the Linux-side spelling of a Windows path, as Docker Desktop, Rancher
/// Desktop and WSL report it, back into a Windows path for display and for
/// opening in Explorer. Non-Windows hosts return the path unchanged.
pub fn display_host_path(source: &str, windows_host: bool) -> String {
    if !windows_host {
        return source.to_string();
    }
    for prefix in ["/run/desktop/mnt/host/", "/mnt/", "/"] {
        if let Some(rest) = source.strip_prefix(prefix) {
            let mut parts = rest.splitn(2, '/');
            let drive = parts.next().unwrap_or("");
            if drive.len() == 1 && drive.chars().all(|c| c.is_ascii_alphabetic()) {
                let tail = parts.next().unwrap_or("").replace('/', "\\");
                let drive = drive.to_ascii_uppercase();
                return if tail.is_empty() {
                    format!("{drive}:\\")
                } else {
                    format!("{drive}:\\{tail}")
                };
            }
        }
    }
    source.to_string()
}

/// Fill `host_path` on every bind mount.
pub fn annotate_host_paths(details: &mut ContainerDetails, windows_host: bool) {
    for mount in &mut details.mounts {
        if mount.kind == "bind" {
            mount.host_path = Some(display_host_path(&mount.source, windows_host));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_wsl_and_desktop_spellings_on_windows() {
        assert_eq!(
            display_host_path("/mnt/c/Users/me/proj", true),
            r"C:\Users\me\proj"
        );
        assert_eq!(
            display_host_path("/run/desktop/mnt/host/d/work", true),
            r"D:\work"
        );
        assert_eq!(display_host_path("/c/Users/me", true), r"C:\Users\me");
        assert_eq!(display_host_path("/mnt/c", true), r"C:\");
        assert_eq!(
            display_host_path("/mnt/wsl/distro", true),
            "/mnt/wsl/distro"
        );
        assert_eq!(
            display_host_path("/mnt/c/Users/me", false),
            "/mnt/c/Users/me"
        );
    }
}
