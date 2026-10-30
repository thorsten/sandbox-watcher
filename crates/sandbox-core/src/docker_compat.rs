//! Map Docker Engine API JSON (also served by Podman) onto the domain model.
//!
//! Kept free of any HTTP client so fixtures in `fixtures/docker/` can be
//! tested directly. The M1 `DockerAdapter` calls these functions.

use std::collections::BTreeMap;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use crate::model::{
    ContainerDetails, ContainerState, Mount, PortBinding, Readiness, SandboxContainer,
};
use crate::sandbox::{HASH_LABEL, PROJECT_LABEL};
use crate::{Error, Result};

fn labels_of(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn strip_name(raw: &str) -> String {
    raw.strip_prefix('/').unwrap_or(raw).to_string()
}

fn strip_localhost(image: &str) -> String {
    image
        .strip_prefix("localhost/")
        .unwrap_or(image)
        .to_string()
}

fn rfc3339(value: &Value) -> Option<DateTime<Utc>> {
    let s = value.as_str()?;
    // Docker reports "0001-01-01T00:00:00Z" for "never started".
    if s.starts_with("0001-") {
        return None;
    }
    DateTime::parse_from_rfc3339(s).ok().map(Into::into)
}

/// One entry of `GET /containers/json`. Returns `Ok(None)` for containers
/// without the sandbox project label.
pub fn container_summary(entry: &Value) -> Result<Option<SandboxContainer>> {
    let labels = labels_of(&entry["Labels"]);
    let Some(project_slug) = labels.get(PROJECT_LABEL).cloned() else {
        return Ok(None);
    };
    let id = entry["Id"]
        .as_str()
        .ok_or_else(|| Error::Protocol("container without Id".into()))?;
    let name = entry["Names"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map(strip_name)
        .unwrap_or_else(|| id.chars().take(12).collect());
    let created_at = entry["Created"]
        .as_i64()
        .and_then(|t| Utc.timestamp_opt(t, 0).single());
    Ok(Some(SandboxContainer {
        id: id.to_string(),
        name,
        project_slug,
        image: strip_localhost(entry["Image"].as_str().unwrap_or_default()),
        state: ContainerState::parse(entry["State"].as_str().unwrap_or("unknown")),
        created_at,
        started_at: None, // only available via inspect
        config_hash: labels.get(HASH_LABEL).cloned(),
        labels,
    }))
}

/// Response of `GET /containers/{id}/json`.
pub fn container_inspect(value: &Value) -> Result<Option<ContainerDetails>> {
    let config = &value["Config"];
    let labels = labels_of(&config["Labels"]);
    let Some(project_slug) = labels.get(PROJECT_LABEL).cloned() else {
        return Ok(None);
    };
    let id = value["Id"]
        .as_str()
        .ok_or_else(|| Error::Protocol("inspect without Id".into()))?;
    let state = &value["State"];
    let container_state = ContainerState::parse(state["Status"].as_str().unwrap_or("unknown"));
    let container = SandboxContainer {
        id: id.to_string(),
        name: value["Name"].as_str().map(strip_name).unwrap_or_default(),
        project_slug,
        image: strip_localhost(config["Image"].as_str().unwrap_or_default()),
        state: container_state,
        created_at: rfc3339(&value["Created"]),
        started_at: if container_state.is_running() {
            rfc3339(&state["StartedAt"])
        } else {
            None
        },
        config_hash: labels.get(HASH_LABEL).cloned(),
        labels,
    };
    let mounts = value["Mounts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| Mount {
                    source: m["Source"].as_str().unwrap_or_default().to_string(),
                    destination: m["Destination"].as_str().unwrap_or_default().to_string(),
                    read_only: !m["RW"].as_bool().unwrap_or(true),
                    kind: m["Type"].as_str().unwrap_or("bind").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let ports = value["NetworkSettings"]["Ports"]
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, bindings)| {
                    let (port, proto) = key.split_once('/')?;
                    let container_port = port.parse().ok()?;
                    let host_port = bindings
                        .as_array()
                        .and_then(|b| b.first())
                        .and_then(|b| b["HostPort"].as_str())
                        .and_then(|p| p.parse().ok());
                    Some(PortBinding {
                        container_port,
                        host_port,
                        protocol: proto.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let env_names = config["Env"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|kv| kv.split_once('=').map(|(k, _)| k).unwrap_or(kv).to_string())
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(ContainerDetails {
        container,
        // Readiness needs an exec against /tmp/.sandbox-ready; the adapter fills it in.
        readiness: Readiness::Unknown,
        mounts,
        ports,
        env_names,
        user: config["User"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(String::from),
        exit_code: if container_state.is_running() {
            None
        } else {
            state["ExitCode"].as_i64()
        },
    }))
}
