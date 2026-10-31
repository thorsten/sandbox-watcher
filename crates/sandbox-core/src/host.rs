//! Which machine a runtime or container lives on, and the container key the
//! UI and Tauri commands use to address a container across machines.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::RuntimeKind;

/// `Local` sorts before every remote host.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Host {
    #[default]
    Local,
    /// An SSH alias, validated by [`validate_alias`].
    Remote(String),
}

impl Host {
    pub fn is_local(&self) -> bool {
        matches!(self, Host::Local)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Host::Local => "local",
            Host::Remote(alias) => alias,
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Host {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        Ok(if value == "local" {
            Host::Local
        } else {
            Host::Remote(value)
        })
    }
}

/// One runtime on one machine.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuntimeId {
    pub host: Host,
    pub kind: RuntimeKind,
}

/// Reject anything `ssh` could read as an option or the remote shell could
/// interpret. `/` is excluded so container keys stay unambiguous.
pub fn validate_alias(alias: &str) -> Result<(), String> {
    if alias.is_empty() || alias.len() > 255 {
        return Err("host alias must be 1 to 255 characters".into());
    }
    if alias.starts_with('-') {
        return Err("host alias must not start with '-'".into());
    }
    if alias == "local" {
        return Err("'local' is reserved for this machine".into());
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-'))
    {
        return Err("host alias may only contain letters, digits, '.', '_', '@' and '-'".into());
    }
    Ok(())
}

/// `<id>` for local containers, `<alias>/<id>` for remote ones.
pub fn container_key(host: &Host, id: &str) -> String {
    match host {
        Host::Local => id.to_string(),
        Host::Remote(alias) => format!("{alias}/{id}"),
    }
}

/// Inverse of [`container_key`]. Container ids and names never contain `/`.
pub fn parse_container_key(key: &str) -> (Host, String) {
    match key.split_once('/') {
        Some((alias, id)) => (Host::Remote(alias.to_string()), id.to_string()),
        None => (Host::Local, key.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_aliases() {
        for ok in ["devbox", "me@vm.example.com", "build-01", "a_b.c"] {
            assert!(validate_alias(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-oProxyCommand=x",
            "local",
            "a b",
            "a/b",
            "a;b",
            "a$b",
            "ä",
            &"x".repeat(256),
        ] {
            assert!(validate_alias(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn container_keys_round_trip() {
        let remote = Host::Remote("devbox".into());
        assert_eq!(container_key(&Host::Local, "abc"), "abc");
        assert_eq!(container_key(&remote, "abc"), "devbox/abc");
        assert_eq!(parse_container_key("abc"), (Host::Local, "abc".into()));
        assert_eq!(parse_container_key("devbox/abc"), (remote, "abc".into()));
    }

    #[test]
    fn host_serialises_as_plain_string() {
        assert_eq!(serde_json::to_string(&Host::Local).unwrap(), "\"local\"");
        assert_eq!(
            serde_json::to_string(&Host::Remote("devbox".into())).unwrap(),
            "\"devbox\""
        );
        let back: Host = serde_json::from_str("\"devbox\"").unwrap();
        assert_eq!(back, Host::Remote("devbox".into()));
        let local: Host = serde_json::from_str("\"local\"").unwrap();
        assert_eq!(local, Host::Local);
    }

    #[test]
    fn local_sorts_first() {
        let mut hosts = [
            Host::Remote("b".into()),
            Host::Local,
            Host::Remote("a".into()),
        ];
        hosts.sort();
        assert_eq!(hosts[0], Host::Local);
        assert_eq!(hosts[1], Host::Remote("a".into()));
    }
}
