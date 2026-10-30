//! Map the Docker fixtures in `fixtures/docker/` onto the domain model.

use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use sandbox_core::docker_compat::{container_inspect, container_summary};
use sandbox_core::model::{ContainerState, MountRole};
use sandbox_core::mounts::classify;
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/docker")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("fixture is valid JSON")
}

#[test]
fn maps_container_list() {
    let list = fixture("containers-list.json");
    let containers: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| container_summary(e).unwrap())
        .collect();
    assert_eq!(containers.len(), 2);

    let shop = &containers[0];
    assert_eq!(shop.name, "sandbox-web-shop");
    assert_eq!(shop.project_slug, "web-shop");
    assert_eq!(
        shop.image, "sandbox-project-web-shop:0.73.1-7f3a",
        "localhost/ prefix stripped"
    );
    assert_eq!(shop.state, ContainerState::Running);
    assert_eq!(shop.config_hash.as_deref(), Some("a1b2c3d4e5f6"));
    assert_eq!(shop.created_at, Utc.timestamp_opt(1761810000, 0).single());

    let billing = &containers[1];
    assert_eq!(billing.state, ContainerState::Exited);
    assert_eq!(billing.project_slug, "billing-api");
}

#[test]
fn ignores_containers_without_sandbox_label() {
    let list = fixture("containers-list-with-foreign.json");
    let entries = list.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let mapped: Vec<_> = entries
        .iter()
        .filter_map(|e| container_summary(e).unwrap())
        .collect();
    assert_eq!(mapped.len(), 2);
    assert!(mapped.iter().all(|c| c.name.starts_with("sandbox-")));
}

#[test]
fn maps_inspect_details() {
    let details = container_inspect(&fixture("container-inspect.json"))
        .unwrap()
        .unwrap();
    let c = &details.container;
    assert_eq!(c.name, "sandbox-web-shop");
    assert_eq!(c.state, ContainerState::Running);
    assert_eq!(
        c.started_at.unwrap().to_rfc3339(),
        "2026-10-30T08:00:01.500+00:00"
    );
    assert!(c.created_at.is_some());

    assert_eq!(details.mounts.len(), 3);
    assert!(!details.mounts[0].read_only);
    assert!(
        details.mounts[2].read_only,
        "runtime files are mounted read-only"
    );

    assert_eq!(details.ports.len(), 2);
    let published = details
        .ports
        .iter()
        .find(|p| p.container_port == 3000)
        .unwrap();
    assert_eq!(published.host_port, Some(3000));
    let exposed = details
        .ports
        .iter()
        .find(|p| p.container_port == 9229)
        .unwrap();
    assert_eq!(exposed.host_port, None);

    assert_eq!(details.user.as_deref(), Some("sandbox"));
    assert!(details.env_names.contains(&"ANTHROPIC_API_KEY".to_string()));
    assert!(
        details.env_names.iter().all(|n| !n.contains('=')),
        "env values must never reach the model"
    );
    assert_eq!(details.exit_code, None);
}

#[test]
fn classifies_fixture_mounts() {
    let mut details = container_inspect(&fixture("container-inspect.json"))
        .unwrap()
        .unwrap();
    assert_eq!(
        details.working_dir.as_deref(),
        Some("/Users/dev/projects/web-shop")
    );
    classify(&mut details);
    let roles: Vec<_> = details.mounts.iter().map(|m| m.role).collect();
    assert_eq!(
        roles,
        vec![MountRole::Project, MountRole::Persisted, MountRole::Runtime]
    );
}

#[test]
fn classifies_fixture_mounts_without_working_dir() {
    let mut details = container_inspect(&fixture("container-inspect.json"))
        .unwrap()
        .unwrap();
    details.working_dir = None;
    classify(&mut details);
    let roles: Vec<_> = details.mounts.iter().map(|m| m.role).collect();
    assert_eq!(
        roles,
        vec![MountRole::Project, MountRole::Persisted, MountRole::Runtime],
        "identity mount wins"
    );
}
