use std::path::PathBuf;

use sandbox_core::apple::{parse_container, parse_stats};
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

#[test]
fn maps_apple_container_stats() {
    let items = fixture("container-stats.json");
    let at = chrono::Utc::now();
    let parsed: Vec<_> = items
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| parse_stats(v, at))
        .collect();
    assert_eq!(parsed.len(), 2);

    let alpha = &parsed[0];
    assert_eq!(alpha.container_id, "sandbox-alpha");
    assert!(
        (alpha.cpu_percent - 379.825_508).abs() < 1e-3,
        "a fraction of one core becomes a percentage, got {}",
        alpha.cpu_percent
    );
    assert_eq!(alpha.memory_used_bytes, 4_992_421_888);
    assert_eq!(alpha.memory_limit_bytes, Some(8_589_934_592));
    assert_eq!(alpha.net_rx_bytes, 411_033_034);
    assert_eq!(alpha.net_tx_bytes, 19_308_651);
    assert_eq!(alpha.block_read_bytes, 822_624_256);
    assert_eq!(alpha.block_write_bytes, 975_872_000);
}

#[test]
fn apple_stats_tolerate_missing_fields() {
    let sample = parse_stats(
        &serde_json::json!({ "id": "sandbox-x" }),
        chrono::Utc::now(),
    )
    .unwrap();
    assert_eq!(sample.cpu_percent, 0.0);
    assert_eq!(sample.memory_limit_bytes, None);
    assert!(parse_stats(&serde_json::json!({}), chrono::Utc::now()).is_none());
}
