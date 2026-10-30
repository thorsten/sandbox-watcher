use std::path::PathBuf;

use sandbox_core::apple::parse_container;
use sandbox_core::model::ContainerState;
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/apple-container")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn maps_apple_container_list() {
    let items = fixture("container-list.json");
    let parsed: Vec<_> = items
        .as_array()
        .unwrap()
        .iter()
        .filter_map(parse_container)
        .collect();
    assert_eq!(parsed.len(), 2);

    let (alpha, mounts) = &parsed[0];
    assert_eq!(alpha.id, "sandbox-alpha");
    assert_eq!(alpha.name, "sandbox-alpha", "Apple uses the id as name");
    assert_eq!(alpha.project_slug, "alpha");
    assert_eq!(alpha.state, ContainerState::Running);
    assert!(alpha.started_at.is_some());
    assert!(alpha.created_at.is_some());
    assert_eq!(mounts.len(), 2);
    assert!(mounts[0].read_only);
    assert!(!mounts[1].read_only);

    let (beta, _) = &parsed[1];
    assert_eq!(
        beta.state,
        ContainerState::Created,
        "stopped without startedDate"
    );
    assert!(beta.started_at.is_none());
}
