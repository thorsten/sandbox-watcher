//! Add blocked destinations to sandbox's `allow_network` list the way
//! `sandbox network allow` does. That command is interactive only, so the
//! watcher performs the same edit itself: it appends entries to the chosen
//! `config.toml` with the formatting of sandbox 0.73.1
//! (`configuration/allowed-network-config-update.ts`), editing the parsed
//! document so comments and other settings survive.
//!
//! Writing is defensive because the file sits in user-controlled trees:
//!
//! - Entries are validated and serialised by `toml_edit`; a value can never
//!   add keys.
//! - Only a missing file counts as an empty config. Unreadable, non-UTF-8 or
//!   unparsable files abort the write untouched.
//! - The project config (`<root>/.sandbox/config.toml`) refuses symbolic
//!   links for `.sandbox` and the file, because project trees may come from a
//!   clone. The global config follows links (dotfile managers) as long as the
//!   target is a regular file; the link itself is kept.
//! - The new content goes to a fresh, uniquely named temporary file created
//!   with `create_new`, so a prepared link cannot redirect it, then replaces
//!   the config with its permissions preserved.
//! - Writers of one directory serialise on an advisory lock, so two allow
//!   dialogs cannot lose each other's entries.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, Item, Value};

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

/// An entry is `host` or `host:port`: lowercase DNS labels, optionally a
/// leading `*.` wildcard, port 1 to 65535. Everything else is refused so
/// the config only ever receives what the Allow dialog showed.
pub fn validate_entry(entry: &str) -> Result<(), String> {
    let (host, port) = match entry.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (entry, None),
    };
    if let Some(port) = port {
        match port.parse::<u16>() {
            Ok(p) if p > 0 && !port.starts_with('+') => {}
            _ => return Err(format!("invalid port in {entry:?}")),
        }
    }
    let host = host.strip_prefix("*.").unwrap_or(host);
    if host.is_empty() || host.len() > 253 {
        return Err(format!("invalid host in {entry:?}"));
    }
    let label_ok = |label: &str| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
    };
    if !host.split('.').all(label_ok) {
        return Err(format!("invalid host in {entry:?}"));
    }
    Ok(())
}

/// Entries in a config's `allow_network` array. A non-array value counts as
/// no entries; a document that does not parse is an error.
pub fn existing_entries(content: &str) -> Result<Vec<String>, String> {
    let doc = parse(content)?;
    Ok(doc
        .get("allow_network")
        .and_then(Item::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default())
}

fn decor_text(raw: Option<&toml_edit::RawString>) -> String {
    raw.and_then(|r| r.as_str()).unwrap_or("").to_string()
}

fn parse(content: &str) -> Result<DocumentMut, String> {
    content
        .parse::<DocumentMut>()
        .map_err(|e| format!("config is not valid TOML: {e}"))
}

/// Append `domains` to the root `allow_network` array, creating it when
/// missing. Multi-line arrays stay multi-line with a trailing comma, the shape
/// sandbox's `injectAllowedDomains` writes; single-line arrays stay single-line.
pub fn inject_allowed_domains(content: &str, domains: &[String]) -> Result<String, String> {
    if domains.is_empty() {
        return Ok(content.to_string());
    }
    let mut doc = parse(content)?;
    let item = doc
        .as_table_mut()
        .entry("allow_network")
        .or_insert_with(|| Item::Value(Value::Array(Array::new())));
    let Some(array) = item.as_array_mut() else {
        return Err("allow_network is not an array".into());
    };
    let multi_line = array.is_empty()
        || array.iter().any(|v| {
            v.decor()
                .prefix()
                .and_then(|p| p.as_str())
                .is_some_and(|p| p.contains('\n'))
        })
        || array.trailing().as_str().is_some_and(|t| t.contains('\n'));
    if multi_line {
        // A comment behind the last value, or behind its comma in the
        // array's trailing text, would swallow the comma we add. Carry it in
        // front of the first new value; sandbox keeps comments behind the
        // comma the same way.
        let mut carried = String::new();
        if let Some(last) = array.iter_mut().last() {
            let suffix = decor_text(last.decor().suffix());
            if suffix.contains('#') {
                carried.push_str(suffix.trim_end());
                last.decor_mut().set_suffix("");
            }
        }
        let trailing = decor_text(Some(array.trailing()));
        if trailing.contains('#') {
            carried.push_str(trailing.trim_end());
        }
        for (i, domain) in domains.iter().enumerate() {
            let prefix = if i == 0 {
                format!("{carried}\n  ")
            } else {
                "\n  ".into()
            };
            array.push_formatted(Value::from(domain.as_str()).decorated(prefix, ""));
        }
        array.set_trailing_comma(true);
        array.set_trailing("\n");
    } else {
        for domain in domains {
            array.push_formatted(Value::from(domain.as_str()).decorated(" ", ""));
        }
    }
    Ok(doc.to_string())
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
/// `target` decides how symbolic links are treated (see the module docs).
pub fn apply(path: &Path, target: ConfigTarget, entries: &[String]) -> io::Result<AllowResult> {
    for entry in entries {
        validate_entry(entry).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    }
    let file = resolve(path, target)?;
    let dir = file
        .parent()
        .ok_or_else(|| invalid_input("config path has no directory"))?;
    let _lock = DirLock::acquire(dir)?;
    let content = read_config(&file)?;
    let existing: BTreeSet<String> = existing_entries(&content)
        .map_err(invalid_data)?
        .into_iter()
        .collect();
    let (already_present, mut added): (Vec<String>, Vec<String>) =
        entries.iter().cloned().partition(|e| existing.contains(e));
    added.dedup();
    if !added.is_empty() {
        let updated = inject_allowed_domains(&content, &added).map_err(invalid_data)?;
        replace_file(&file, updated.as_bytes())?;
    }
    Ok(AllowResult {
        path: path.display().to_string(),
        added,
        already_present,
    })
}

fn invalid_input(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}

fn invalid_data(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn is_symlink(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

/// The file to read and replace, with every directory link resolved.
fn resolve(path: &Path, target: ConfigTarget) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_input("config path has no directory"))?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid_input("config path has no file name"))?;
    match target {
        ConfigTarget::Project => {
            for p in [parent, path] {
                if is_symlink(p) {
                    return Err(invalid_input(format!(
                        "{} is a symbolic link; refusing to write the project config",
                        p.display()
                    )));
                }
            }
            if !parent.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{} is not a directory", parent.display()),
                ));
            }
            Ok(parent.canonicalize()?.join(name))
        }
        ConfigTarget::Global => {
            std::fs::create_dir_all(parent)?;
            let dir = parent.canonicalize()?;
            let file = dir.join(name);
            match file.symlink_metadata() {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(file),
                Err(e) => Err(e),
                Ok(meta) if meta.is_file() => Ok(file),
                Ok(meta) if meta.file_type().is_symlink() => {
                    let real = file.canonicalize().map_err(|e| {
                        io::Error::new(
                            e.kind(),
                            format!("{} points to an unreadable target: {e}", file.display()),
                        )
                    })?;
                    if !real.is_file() {
                        return Err(invalid_input(format!(
                            "{} does not point to a regular file",
                            file.display()
                        )));
                    }
                    Ok(real)
                }
                Ok(_) => Err(invalid_input(format!(
                    "{} is not a regular file",
                    file.display()
                ))),
            }
        }
    }
}

/// Only a missing file is an empty config.
fn read_config(file: &Path) -> io::Result<String> {
    match std::fs::read(file) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| invalid_data(format!("{} is not UTF-8", file.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `content` to a fresh temporary file next to `file` and rename it
/// over `file`, keeping `file`'s permissions. `create_new` never follows a
/// prepared link of the same name; the name is unpredictable anyway.
fn replace_file(file: &Path, content: &[u8]) -> io::Result<()> {
    let dir = file
        .parent()
        .ok_or_else(|| invalid_input("config path has no directory"))?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.toml");
    let permissions = match std::fs::metadata(file) {
        Ok(meta) => Some(meta.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(
        ".{name}.{}.{nanos:x}.{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut handle = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let result = (|| {
        handle.write_all(content)?;
        if let Some(perm) = permissions {
            handle.set_permissions(perm)?;
        }
        handle.sync_all()?;
        drop(handle);
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Advisory lock on the config's directory, released on drop. On Unix the
/// directory itself is locked, which needs no extra file; Windows cannot lock
/// a directory handle, so a lock file is used there.
struct DirLock(#[allow(dead_code)] File);

impl DirLock {
    fn acquire(dir: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        let handle = File::open(dir)?;
        #[cfg(not(unix))]
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(".config.toml.lock"))?;
        handle.lock()?;
        Ok(Self(handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ports(list: &[u16]) -> BTreeSet<u16> {
        list.iter().copied().collect()
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    static DIRS: AtomicUsize = AtomicUsize::new(0);

    /// A fresh directory per test; removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let n = DIRS.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("sw-allow-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
    fn validates_entries() {
        for ok in [
            "example.com",
            "a-b.example.com:8443",
            "*.example.com",
            "localhost",
            "x_y.io:1",
        ] {
            assert_eq!(validate_entry(ok), Ok(()), "{ok}");
        }
        for bad in [
            "",
            "Example.com",
            "example.com:0",
            "example.com:65536",
            "example.com:x",
            "example.com:443:1",
            "exa mple.com",
            "a.b\"]\nfoo = 1 #",
            "a.b\",\"evil.com",
            "::1",
            "[::1]:443",
            "-example.com",
            "ex*ample.com",
            "http://example.com",
            "example.com/path",
        ] {
            assert!(validate_entry(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn injects_like_sandbox_when_array_exists() {
        let content = "env = [\"FOO\"]\nallow_network = [\n  \"registry.npmjs.org\",\n  \"github.com\" # code host\n]\nclipboard = \"disabled\"\n";
        let out = inject_allowed_domains(content, &strings(&["api.example.com"])).unwrap();
        assert_eq!(
            out,
            "env = [\"FOO\"]\nallow_network = [\n  \"registry.npmjs.org\",\n  \"github.com\", # code host\n  \"api.example.com\",\n]\nclipboard = \"disabled\"\n"
        );
    }

    #[test]
    fn injects_into_empty_array_and_appends_when_missing() {
        assert_eq!(
            inject_allowed_domains("allow_network = [\n]\n", &strings(&["a.b"])).unwrap(),
            "allow_network = [\n  \"a.b\",\n]\n"
        );
        assert_eq!(
            inject_allowed_domains("allow_network = []", &strings(&["a.b"])).unwrap(),
            "allow_network = [\n  \"a.b\",\n]\n"
        );
        assert_eq!(
            inject_allowed_domains("env = []\n", &strings(&["a.b", "c.d:5432"])).unwrap(),
            "env = []\nallow_network = [\n  \"a.b\",\n  \"c.d:5432\",\n]\n"
        );
        assert_eq!(inject_allowed_domains("x = 1", &[]).unwrap(), "x = 1");
    }

    #[test]
    fn keeps_single_line_arrays_single_line() {
        assert_eq!(
            inject_allowed_domains("allow_network = [\"a.b\"] # hosts\n", &strings(&["c.d"]))
                .unwrap(),
            "allow_network = [\"a.b\", \"c.d\"] # hosts\n"
        );
    }

    #[test]
    fn comments_with_closing_brackets_do_not_confuse_the_parser() {
        let content = "allow_network = [\n  \"a.b\", # see [docs] and ]\n] # end ]\nx = 1\n";
        let out = inject_allowed_domains(content, &strings(&["c.d"])).unwrap();
        assert_eq!(
            out,
            "allow_network = [\n  \"a.b\", # see [docs] and ]\n  \"c.d\",\n] # end ]\nx = 1\n"
        );
        assert_eq!(existing_entries(&out).unwrap(), vec!["a.b", "c.d"]);
    }

    #[test]
    fn adds_to_the_root_table_when_tables_exist() {
        let content = "# allow_network = [\"old\"]\n[section]\nallow_network_note = \"x\"\nallow_network = [\"nested\"]\n";
        let out = inject_allowed_domains(content, &strings(&["a.b"])).unwrap();
        let doc: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(existing_entries(&out).unwrap(), vec!["a.b"]);
        assert_eq!(
            doc["section"]["allow_network"].as_array().unwrap().len(),
            1,
            "{out}"
        );
        assert_eq!(doc["section"]["allow_network_note"].as_str(), Some("x"));
        assert!(
            out.contains("# allow_network = [\"old\"]\n[section]\n"),
            "{out}"
        );
    }

    #[test]
    fn rejects_broken_or_mistyped_configs() {
        assert!(inject_allowed_domains("allow_network = [\n", &strings(&["a.b"])).is_err());
        assert!(inject_allowed_domains("allow_network = \"a.b\"", &strings(&["c.d"])).is_err());
        assert!(existing_entries("allow_network = [").is_err());
        assert_eq!(
            existing_entries("allow_network = \"x\"").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn manipulated_entries_are_escaped_not_injected() {
        let out = inject_allowed_domains("", &strings(&["a.b\"]\nfoo = 1 #"])).unwrap();
        let doc: toml::Value = toml::from_str(&out).unwrap();
        assert!(doc.get("foo").is_none(), "{out}");
        assert_eq!(existing_entries(&out).unwrap(), vec!["a.b\"]\nfoo = 1 #"]);
    }

    #[test]
    fn apply_refuses_invalid_entries_before_touching_the_file() {
        let t = TempDir::new();
        let path = t.path("config.toml");
        let r = apply(
            &path,
            ConfigTarget::Global,
            &strings(&["ok.io", "a.b\"]\nfoo = 1 #"]),
        );
        assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        assert!(!path.exists());
    }

    #[test]
    fn reads_existing_entries_and_applies_idempotently() {
        let t = TempDir::new();
        let path = t.path("config.toml");
        std::fs::write(&path, "allow_network = [\n  \"github.com\",\n]\n").unwrap();
        let r = apply(
            &path,
            ConfigTarget::Global,
            &strings(&["github.com", "api.example.com"]),
        )
        .unwrap();
        assert_eq!(r.added, vec!["api.example.com"]);
        assert_eq!(r.already_present, vec!["github.com"]);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            existing_entries(&content).unwrap(),
            vec!["github.com", "api.example.com"]
        );
        let r2 = apply(&path, ConfigTarget::Global, &strings(&["api.example.com"])).unwrap();
        assert!(r2.added.is_empty());
        assert!(
            std::fs::read_dir(&t.0).unwrap().count() == 1,
            "no leftovers"
        );
    }

    #[test]
    fn missing_file_counts_as_empty_but_other_read_errors_abort() {
        let t = TempDir::new();
        let path = t.path("sub/config.toml");
        apply(&path, ConfigTarget::Global, &strings(&["a.b"])).unwrap();
        assert_eq!(
            existing_entries(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            vec!["a.b"]
        );

        let bad = t.path("bad.toml");
        std::fs::write(&bad, b"allow_network = [\n  \"a.b\"\xff\n]\n").unwrap();
        let err = apply(&bad, ConfigTarget::Global, &strings(&["c.d"])).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(
            std::fs::read(&bad).unwrap(),
            b"allow_network = [\n  \"a.b\"\xff\n]\n"
        );

        let broken = t.path("broken.toml");
        std::fs::write(&broken, "allow_network = [\n").unwrap();
        let err = apply(&broken, ConfigTarget::Global, &strings(&["c.d"])).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(
            std::fs::read_to_string(&broken).unwrap(),
            "allow_network = [\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepared_temporary_symlink_is_not_followed() {
        let t = TempDir::new();
        let victim = t.path("victim");
        std::fs::write(&victim, "keep me").unwrap();
        let path = t.path("config.toml");
        std::os::unix::fs::symlink(&victim, t.path("config.toml.tmp")).unwrap();
        apply(&path, ConfigTarget::Global, &strings(&["a.b"])).unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
        assert_eq!(
            existing_entries(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            vec!["a.b"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn project_config_refuses_symlinked_directory_and_file() {
        let t = TempDir::new();
        let elsewhere = t.path("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("config.toml"), "x = 1\n").unwrap();
        let root = t.path("project");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join(".sandbox")).unwrap();
        let path = project_config_path(&root);
        let err = apply(&path, ConfigTarget::Project, &strings(&["a.b"])).unwrap_err();
        assert!(err.to_string().contains("symbolic link"), "{err}");
        assert_eq!(
            std::fs::read_to_string(elsewhere.join("config.toml")).unwrap(),
            "x = 1\n"
        );

        let root2 = t.path("project2");
        std::fs::create_dir_all(root2.join(".sandbox")).unwrap();
        std::os::unix::fs::symlink(
            elsewhere.join("config.toml"),
            root2.join(".sandbox/config.toml"),
        )
        .unwrap();
        let err = apply(
            &project_config_path(&root2),
            ConfigTarget::Project,
            &strings(&["a.b"]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("symbolic link"), "{err}");
        assert_eq!(
            std::fs::read_to_string(elsewhere.join("config.toml")).unwrap(),
            "x = 1\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn global_config_follows_symlinks_to_regular_files_only() {
        let t = TempDir::new();
        let dotfiles = t.path("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        std::fs::write(dotfiles.join("config.toml"), "x = 1\n").unwrap();
        std::os::unix::fs::symlink(&dotfiles, t.path("sandbox")).unwrap();
        let through_dir = t.path("sandbox/config.toml");
        apply(&through_dir, ConfigTarget::Global, &strings(&["a.b"])).unwrap();
        let written = std::fs::read_to_string(dotfiles.join("config.toml")).unwrap();
        assert_eq!(written, "x = 1\nallow_network = [\n  \"a.b\",\n]\n");
        assert!(
            t.path("sandbox")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let link = t.path("link.toml");
        std::os::unix::fs::symlink(dotfiles.join("config.toml"), &link).unwrap();
        apply(&link, ConfigTarget::Global, &strings(&["c.d"])).unwrap();
        assert!(
            link.symlink_metadata().unwrap().file_type().is_symlink(),
            "link replaced"
        );
        assert_eq!(
            existing_entries(&std::fs::read_to_string(dotfiles.join("config.toml")).unwrap())
                .unwrap(),
            vec!["a.b", "c.d"]
        );

        let to_dir = t.path("dir.toml");
        std::os::unix::fs::symlink(&dotfiles, &to_dir).unwrap();
        assert!(apply(&to_dir, ConfigTarget::Global, &strings(&["e.f"])).is_err());
        let dangling = t.path("dangling.toml");
        std::os::unix::fs::symlink(t.path("nowhere"), &dangling).unwrap();
        assert!(apply(&dangling, ConfigTarget::Global, &strings(&["e.f"])).is_err());
        assert!(!t.path("nowhere").exists());
    }

    #[cfg(unix)]
    #[test]
    fn keeps_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempDir::new();
        let path = t.path("config.toml");
        std::fs::write(&path, "x = 1\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        apply(&path, ConfigTarget::Global, &strings(&["a.b"])).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn concurrent_writers_lose_nothing() {
        let t = TempDir::new();
        let path = t.path("config.toml");
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for j in 0..5 {
                        apply(
                            &path,
                            ConfigTarget::Global,
                            &strings(&[&format!("h{i}-{j}.io")]),
                        )
                        .unwrap();
                    }
                })
            })
            .collect();
        for th in threads {
            th.join().unwrap();
        }
        let entries = existing_entries(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(entries.len(), 40, "{entries:?}");
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
