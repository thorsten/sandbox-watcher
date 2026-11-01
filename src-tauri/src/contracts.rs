//! Serialisation contract for the types that live in the Tauri shell, the
//! counterpart of `crates/sandbox-core/tests/contracts.rs`. The fixtures
//! are checked against `src/types.ts` by `pnpm check:contracts`; regenerate
//! them with `SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-watcher`.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

use crate::AllowPreview;
use crate::updates::{Progress, UpdateInfo};

const UPDATE_ENV: &str = "SANDBOX_WATCHER_UPDATE_CONTRACTS";

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../fixtures/contracts/{name}.json"))
}

fn contract<T: Serialize>(name: &str, samples: Vec<T>) {
    let documents: Vec<Value> = samples
        .iter()
        .map(|s| serde_json::to_value(s).expect("serialise"))
        .collect();
    let mut text =
        serde_json::to_string_pretty(&json!({ "type": name, "samples": documents })).unwrap();
    text.push('\n');
    let path = fixture_path(name);
    if std::env::var_os(UPDATE_ENV).is_some_and(|v| v == "1") {
        std::fs::write(&path, &text).expect("write fixture");
        return;
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run with {UPDATE_ENV}=1", path.display()));
    assert_eq!(
        on_disk,
        text,
        "{} is out of date; run `{UPDATE_ENV}=1 cargo test -p sandbox-watcher`, then \
         `pnpm check:contracts`, and commit both",
        path.display()
    );
}

#[test]
fn update_info() {
    contract(
        "UpdateInfo",
        vec![
            UpdateInfo {
                version: "0.7.0".into(),
                current_version: "0.6.0".into(),
                notes: Some("## Added\n\n- Things".into()),
                date: Some("2026-11-01T10:00:00Z".into()),
            },
            UpdateInfo {
                version: "0.7.0".into(),
                current_version: "0.6.0".into(),
                notes: None,
                date: None,
            },
        ],
    );
}

#[test]
fn update_progress() {
    contract(
        "UpdateProgress",
        vec![
            Progress {
                downloaded: 1_048_576,
                total: Some(12_582_912),
            },
            Progress {
                downloaded: 0,
                total: None,
            },
        ],
    );
}

#[test]
fn allow_preview() {
    contract(
        "AllowPreview",
        vec![
            AllowPreview {
                entries: vec!["api.example.com:443".into(), "cdn.example.com:443".into()],
                global_path: Some("/Users/dev/.sandbox/config.toml".into()),
                project_path: Some("/Users/dev/projects/web-shop/.sandbox/config.toml".into()),
                already_in_global: vec!["cdn.example.com:443".into()],
                already_in_project: vec![],
            },
            AllowPreview {
                entries: vec![],
                global_path: None,
                project_path: None,
                already_in_global: vec![],
                already_in_project: vec![],
            },
        ],
    );
}
