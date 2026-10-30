//! Add blocked destinations to sandbox's `allow_network` list the way
//! `sandbox network allow` does. That command is interactive only, so the
//! watcher performs the same edit itself: it appends entries to the chosen
//! `config.toml` with the exact rewriting rules of sandbox 0.73.1
//! (`configuration/allowed-network-config-update.ts`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Ports sandbox allows when an entry names none. `allowed-network-formatting.ts`.
pub const DEFAULT_PORTS: [u16; 2] = [80, 443];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigTarget {
    Global,
    Project,
}

/// Build `allow_network` entries for a destination and the ports it was
/// blocked on. Default ports collapse into the bare host; other ports become
/// `host:port`, one entry each, the form sandbox documents.
pub fn entries_for(host: &str, ports: &BTreeSet<u16>) -> Vec<String> {
    let host = canonical_host(host);
    if host.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let defaults: BTreeSet<u16> = DEFAULT_PORTS.into_iter().collect();
    if ports.is_empty() || ports.iter().any(|p| defaults.contains(p)) {
        out.push(host.clone());
    }
    for port in ports.iter().filter(|p| !defaults.contains(p) && **p != 0) {
        out.push(format!("{host}:{port}"));
    }
    out
}

/// Lowercase, strip trailing dots and IPv6 brackets. `canonicalizeHost`.
pub fn canonical_host(host: &str) -> String {
    let mut h = host.trim().to_ascii_lowercase();
    while h.ends_with('.') {
        h.pop();
    }
    if h.starts_with('[') && h.ends_with(']') {
        h = h[1..h.len() - 1].to_string();
    }
    h
}

/// Entries already present in a config's `allow_network` array.
pub fn existing_entries(content: &str) -> Vec<String> {
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return vec![];
    };
    value
        .get("allow_network")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn strip_inline_comment(line: &str) -> String {
    match line.find('#') {
        Some(i) => line[..i].trim_end().to_string(),
        None => line.trim_end().to_string(),
    }
}

fn append_trailing_comma(line: &str) -> String {
    match line.find('#') {
        Some(i) => {
            // Keep the whitespace in front of the comment, like sandbox does.
            let value = line[..i].trim_end();
            format!("{value},{}", &line[value.len()..])
        }
        None => format!("{line},"),
    }
}

fn inject_into_prefix(prefix: &str, domain_lines: &str) -> String {
    let mut lines: Vec<String> = prefix.trim_end().split('\n').map(String::from).collect();
    // Last line that holds a value (not blank, not the opening bracket).
    let mut last_value: Option<usize> = None;
    for index in (0..lines.len()).rev() {
        let value_only = strip_inline_comment(&lines[index]);
        if value_only.trim().is_empty() {
            continue;
        }
        if value_only.ends_with('[') {
            break;
        }
        last_value = Some(index);
        break;
    }
    if let Some(index) = last_value
        && !strip_inline_comment(&lines[index]).ends_with(',')
    {
        lines[index] = append_trailing_comma(&lines[index]);
    }
    format!("{}\n{domain_lines}\n]", lines.join("\n"))
}

/// Port of sandbox's `injectAllowedDomains`.
pub fn inject_allowed_domains(content: &str, domains: &[String]) -> String {
    if domains.is_empty() {
        return content.to_string();
    }
    let domain_lines = domains
        .iter()
        .map(|d| format!("  \"{d}\","))
        .collect::<Vec<_>>()
        .join("\n");
    // Find a line starting with `allow_network = [` (leading whitespace allowed).
    let mut search_from = 0;
    let mut array_start: Option<usize> = None;
    while let Some(rel) = content[search_from..].find("allow_network") {
        let pos = search_from + rel;
        let line_start = content[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let leading_ok = content[line_start..pos].chars().all(char::is_whitespace);
        let rest = &content[pos + "allow_network".len()..];
        let after_eq = rest.trim_start().strip_prefix('=').map(str::trim_start);
        if leading_ok && after_eq.is_some_and(|r| r.starts_with('[')) {
            array_start = Some(line_start);
            break;
        }
        search_from = pos + "allow_network".len();
    }
    match array_start {
        Some(start) => {
            let bracket = start + content[start..].find('[').unwrap();
            let Some(close_rel) = content[bracket..].find(']') else {
                return format!("{content}\nallow_network = [\n{domain_lines}\n]");
            };
            let close = bracket + close_rel;
            let prefix = &content[start..close];
            format!(
                "{}{}{}",
                &content[..start],
                inject_into_prefix(prefix, &domain_lines),
                &content[close + 1..]
            )
        }
        None => format!("{content}\nallow_network = [\n{domain_lines}\n]"),
    }
}

/// `~/.config/sandbox/config.toml` (or `SANDBOX_CONFIG_DIR`, `XDG_CONFIG_HOME`,
/// `%APPDATA%`), mirroring `config-paths.ts`.
pub fn global_config_path(
    os: &str,
    sandbox_config_dir: Option<&str>,
    xdg_config_home: Option<&str>,
    appdata: Option<&str>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(dir) = sandbox_config_dir.filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join("config.toml"));
    }
    crate::config::config_path(os, xdg_config_home, appdata, home)
}

pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".sandbox").join("config.toml")
}

/// Sandbox only offers the project config when `.sandbox/` exists.
pub fn project_config_available(project_root: &Path) -> bool {
    project_root.join(".sandbox").is_dir()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowResult {
    pub path: String,
    pub added: Vec<String>,
    pub already_present: Vec<String>,
}

/// Append entries to the config file at `path`, skipping ones already there.
pub fn apply(path: &Path, entries: &[String]) -> std::io::Result<AllowResult> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let existing: BTreeSet<String> = existing_entries(&content).into_iter().collect();
    let (already_present, added): (Vec<String>, Vec<String>) =
        entries.iter().cloned().partition(|e| existing.contains(e));
    let mut added = added;
    added.dedup();
    if !added.is_empty() {
        let updated = inject_allowed_domains(&content, &added);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, updated)?;
        std::fs::rename(&tmp, path)?;
    }
    Ok(AllowResult {
        path: path.display().to_string(),
        added,
        already_present,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ports(list: &[u16]) -> BTreeSet<u16> {
        list.iter().copied().collect()
    }

    #[test]
    fn entries_collapse_default_ports() {
        assert_eq!(
            entries_for("Example.COM.", &ports(&[443])),
            vec!["example.com"]
        );
        assert_eq!(
            entries_for("db.example.com", &ports(&[5432])),
            vec!["db.example.com:5432"]
        );
        assert_eq!(
            entries_for("x.io", &ports(&[80, 8443])),
            vec!["x.io", "x.io:8443"]
        );
        assert_eq!(entries_for("dns.only", &ports(&[])), vec!["dns.only"]);
        assert!(entries_for("  ", &ports(&[443])).is_empty());
    }

    #[test]
    fn injects_like_sandbox_when_array_exists() {
        let content = "env = [\"FOO\"]\nallow_network = [\n  \"registry.npmjs.org\",\n  \"github.com\" # code host\n]\nclipboard = \"disabled\"\n";
        let out = inject_allowed_domains(content, &["api.example.com".into()]);
        assert_eq!(
            out,
            "env = [\"FOO\"]\nallow_network = [\n  \"registry.npmjs.org\",\n  \"github.com\", # code host\n  \"api.example.com\",\n]\nclipboard = \"disabled\"\n"
        );
    }

    #[test]
    fn injects_into_empty_array_and_appends_when_missing() {
        assert_eq!(
            inject_allowed_domains("allow_network = [\n]\n", &["a.b".into()]),
            "allow_network = [\n  \"a.b\",\n]\n"
        );
        assert_eq!(
            inject_allowed_domains("allow_network = []", &["a.b".into()]),
            "allow_network = [\n  \"a.b\",\n]"
        );
        assert_eq!(
            inject_allowed_domains("env = []\n", &["a.b".into(), "c.d:5432".into()]),
            "env = []\n\nallow_network = [\n  \"a.b\",\n  \"c.d:5432\",\n]"
        );
        assert_eq!(inject_allowed_domains("x = 1", &[]), "x = 1");
    }

    #[test]
    fn ignores_commented_or_nested_mentions() {
        let content = "# allow_network = [\"old\"]\n[section]\nallow_network_note = \"x\"\n";
        let out = inject_allowed_domains(content, &["a.b".into()]);
        assert!(out.ends_with("\nallow_network = [\n  \"a.b\",\n]"), "{out}");
    }

    #[test]
    fn reads_existing_entries_and_applies_idempotently() {
        let dir = std::env::temp_dir().join(format!("sw-allow-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "allow_network = [\n  \"github.com\",\n]\n").unwrap();
        let r = apply(&path, &["github.com".into(), "api.example.com".into()]).unwrap();
        assert_eq!(r.added, vec!["api.example.com"]);
        assert_eq!(r.already_present, vec!["github.com"]);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            existing_entries(&content),
            vec!["github.com", "api.example.com"]
        );
        let r2 = apply(&path, &["api.example.com".into()]).unwrap();
        assert!(r2.added.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn config_paths_follow_sandbox() {
        let home = Path::new("/home/me");
        assert_eq!(
            global_config_path("linux", Some("/etc/sb"), None, None, Some(home)),
            Some(PathBuf::from("/etc/sb/config.toml"))
        );
        assert_eq!(
            global_config_path("macos", None, None, None, Some(home)),
            Some(PathBuf::from("/home/me/.config/sandbox/config.toml"))
        );
        assert_eq!(
            project_config_path(Path::new("/p")),
            PathBuf::from("/p/.sandbox/config.toml")
        );
    }
}
