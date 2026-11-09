//! What one refresh cycle costs a container in `exec` calls.
//!
//! Every exec starts a process inside the container (and `runc init` on the
//! host), so the number per cycle, not the bytes read, is what loads an idle
//! sandbox. The budget: one exec per running container on an ordinary
//! cycle (the status probe, which also carries session clocks and the
//! transcript listing), one more on a cycle that reads the network logs,
//! and one chunk read per transcript that grew.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use sandbox_core::RuntimeAdapter;
use sandbox_core::adapter::ContainerFilter;
use sandbox_core::mock::MockAdapter;
use sandbox_core::overview::{DataSource, HostedAdapter, load_many};
use sandbox_core::refresh::{collect_all_insights, collect_all_network};
use sandbox_core::transcripts::TranscriptCache;
use sandbox_core::usage::default_prices;

/// Execs per container id in the mock's log, named by the script's `$0`.
fn per_container(mock: &MockAdapter) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (id, command) in mock.exec_log() {
        let label = match command.as_slice() {
            [sh, flag, _, sentinel, ..] if sh == "sh" && flag == "-c" => sentinel.clone(),
            other => other.join(" "),
        };
        out.entry(id).or_default().push(label);
    }
    out
}

/// One background refresh as the watcher runs it: overview with probes,
/// then transcript insights for the history, then the network logs when due.
async fn cycle(mock: &MockAdapter, cache: &TranscriptCache, network: bool) {
    let adapter: Arc<dyn RuntimeAdapter> = Arc::new(mock.clone());
    let hosted = HostedAdapter::local(adapter.clone());
    let overview = load_many(std::slice::from_ref(&hosted), DataSource::Mock, vec![]).await;
    let resolve = |key: &str| {
        overview
            .containers
            .iter()
            .find(|c| c.key == key)
            .map(|c| (adapter.clone(), c.container.id.clone()))
    };
    collect_all_insights(&overview, &resolve, cache, &default_prices()).await;
    if network {
        collect_all_network(&[hosted], &overview, Utc::now()).await;
    }
}

#[tokio::test]
async fn a_steady_cycle_costs_one_exec_per_running_container() {
    let mock = MockAdapter::demo();
    let cache = TranscriptCache::default();
    // First cycle: the transcript is read once (chunk read); that is allowed.
    cycle(&mock, &cache, false).await;
    mock.clear_exec_log();

    cycle(&mock, &cache, false).await;
    let execs = per_container(&mock);
    assert_eq!(execs.len(), 3, "only running containers are touched");
    for (id, commands) in &execs {
        assert_eq!(
            commands,
            &vec!["sandbox-watcher-probe".to_string()],
            "{id}: one steady cycle"
        );
    }
}

#[tokio::test]
async fn a_cycle_with_network_logs_costs_two_execs_per_running_container() {
    let mock = MockAdapter::demo();
    let cache = TranscriptCache::default();
    cycle(&mock, &cache, false).await;
    mock.clear_exec_log();

    cycle(&mock, &cache, true).await;
    for (id, commands) in &per_container(&mock) {
        assert_eq!(
            commands,
            &vec![
                "sandbox-watcher-probe".to_string(),
                "sandbox-watcher-network".to_string()
            ],
            "{id}: one network cycle"
        );
    }
}

#[tokio::test]
async fn a_grown_transcript_costs_one_chunk_read() {
    let mock = MockAdapter::demo();
    let cache = TranscriptCache::default();
    cycle(&mock, &cache, false).await;
    mock.clear_exec_log();
    mock.append_transcript(
        r#"{"type":"assistant","message":{"model":"claude-sonnet-4-5-20250929","id":"msg_09","usage":{"input_tokens":1,"output_tokens":1},"content":[{"type":"text","text":"more"}]},"timestamp":"2026-10-30T10:00:30Z"}"#,
    );
    cycle(&mock, &cache, false).await;
    for (id, commands) in &per_container(&mock) {
        assert_eq!(
            commands,
            &vec![
                "sandbox-watcher-probe".to_string(),
                "sandbox-watcher-chunk".to_string()
            ],
            "{id}: probe plus the appended bytes"
        );
    }
}

#[tokio::test]
async fn stopped_containers_are_never_probed() {
    let mock = MockAdapter::demo();
    let cache = TranscriptCache::default();
    cycle(&mock, &cache, true).await;
    let stopped = mock
        .list_containers(&ContainerFilter::all())
        .await
        .unwrap()
        .into_iter()
        .find(|c| !c.state.is_running())
        .expect("demo has a stopped container");
    assert!(!per_container(&mock).contains_key(&stopped.id));
}
