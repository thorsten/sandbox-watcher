//! Deterministic workloads shared by `benches/perf.rs` (measurement) and
//! `tests/budgets.rs` (budgets). Everything runs against `MockAdapter`, so
//! the numbers describe the watcher's own work, not a container runtime.
//! `scripts/bench-runtime.sh` is the optional variant against real Docker.
//!
//! Included from both places with `#[path]`; keep it free of test-only or
//! bench-only dependencies.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use sandbox_core::RuntimeAdapter;
use sandbox_core::activity::{self, AgentInsight, ContainerActivity};
use sandbox_core::files::FileTouch;
use sandbox_core::history::{ContainerRecord, History, HistoryQuery, SessionRecord, UsageScope};
use sandbox_core::host::Host;
use sandbox_core::mock::{MockAdapter, mock_container};
use sandbox_core::model::{AgentKind, Readiness, RuntimeKind, Session};
use sandbox_core::overview::{
    ContainerOverview, DataSource, HostedAdapter, Overview, RuntimeStatus, load_many,
};
use sandbox_core::transcripts::TranscriptCache;
use sandbox_core::usage::{CostSource, PriceTable, Usage, default_prices};

/// Name of the mock container whose transcript grows.
pub const TRANSCRIPT_CONTAINER: &str = "sandbox-web-shop";

/// The Claude Code session the mock reports for that container.
pub fn claude_session() -> Vec<Session> {
    vec![Session {
        pid: 4242,
        command: "claude".into(),
        agent: AgentKind::ClaudeCode,
    }]
}

pub fn prices() -> PriceTable {
    default_prices()
}

fn clock(second: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 30, 10, 0, 0).unwrap() + Duration::seconds(second)
}

/// One assistant line in Claude Code's transcript shape: a tool call with
/// usage, about 430 bytes, timestamps one second apart. Every fifth line
/// edits a file so the file aggregation has work too.
pub fn claude_line(i: usize) -> String {
    let at = clock(1 + i as i64).format("%Y-%m-%dT%H:%M:%SZ");
    let (tool, input) = if i.is_multiple_of(5) {
        (
            "Edit",
            format!(
                r#"{{"file_path":"/Users/dev/projects/web-shop/src/module{}.ts","old_string":"a","new_string":"b"}}"#,
                i % 37
            ),
        )
    } else {
        (
            "Read",
            format!(
                r#"{{"file_path":"/Users/dev/projects/web-shop/src/module{}.ts"}}"#,
                i % 37
            ),
        )
    };
    format!(
        r#"{{"type":"assistant","message":{{"model":"claude-sonnet-4-5-20250929","id":"msg_{i:08}","role":"assistant","usage":{{"input_tokens":120,"cache_creation_input_tokens":800,"cache_read_input_tokens":64000,"output_tokens":90}},"content":[{{"type":"text","text":"Step {i}: looking at the module before changing it."}},{{"type":"tool_use","id":"toolu_{i:08}","name":"{tool}","input":{input}}}]}},"timestamp":"{at}"}}"#
    )
}

/// The result line that pairs with [`claude_line`].
pub fn claude_result_line(i: usize) -> String {
    let at = clock(1 + i as i64).format("%Y-%m-%dT%H:%M:%SZ");
    format!(
        r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_{i:08}","content":"ok"}}]}},"timestamp":"{at}"}}"#
    )
}

/// Append `lines` transcript lines (alternating call and result).
pub fn grow_transcript(adapter: &MockAdapter, from: usize, lines: usize) {
    for i in from..from + lines {
        if i % 2 == 0 {
            adapter.append_transcript(&claude_line(i / 2));
        } else {
            adapter.append_transcript(&claude_result_line(i / 2));
        }
    }
}

/// The demo adapter with a transcript of `lines` more lines.
pub fn transcript_adapter(lines: usize) -> MockAdapter {
    let adapter = MockAdapter::demo();
    grow_transcript(&adapter, 0, lines);
    adapter
}

/// What the watcher does for one container on every refresh: usage and
/// files from whatever was appended since the last refresh.
pub async fn refresh_insights(
    adapter: &Arc<dyn RuntimeAdapter>,
    cache: &TranscriptCache,
    prices: &PriceTable,
) -> Vec<AgentInsight> {
    activity::collect_insights(
        adapter,
        TRANSCRIPT_CONTAINER,
        TRANSCRIPT_CONTAINER,
        &claude_session(),
        cache,
        prices,
    )
    .await
}

/// What the Activity panel asks for: processes, events, usage, files, git.
pub async fn activity_panel(
    adapter: &Arc<dyn RuntimeAdapter>,
    cache: &TranscriptCache,
    prices: &PriceTable,
) -> ContainerActivity {
    activity::collect(
        adapter,
        TRANSCRIPT_CONTAINER,
        TRANSCRIPT_CONTAINER,
        &claude_session(),
        Utc::now(),
        prices,
        cache,
    )
    .await
}

/// `count` running containers, one project each, with the demo sessions.
pub fn containers_adapter(count: usize) -> MockAdapter {
    let adapter = MockAdapter::empty();
    for i in 0..count {
        adapter.add(mock_container(
            &format!("project-{i:04}"),
            1,
            "running",
            "a1b2c3d4e5f6",
        ));
    }
    adapter
}

/// One overview refresh over the local adapter.
pub async fn overview(adapter: &MockAdapter) -> Overview {
    let hosted = HostedAdapter::local(Arc::new(adapter.clone()));
    load_many(&[hosted], DataSource::Mock, vec![]).await
}

/// A synthetic overview with `count` running containers of two sessions
/// each, without going through an adapter.
pub fn synthetic_overview(count: usize, now: DateTime<Utc>) -> Overview {
    let containers = (0..count)
        .map(|i| {
            let mock = mock_container(&format!("project-{i:04}"), 1, "running", "a1b2c3d4e5f6");
            let mut container = mock.details.container;
            container.started_at = Some(now - Duration::minutes(30));
            ContainerOverview {
                key: container.id.clone(),
                container,
                host: Host::Local,
                runtime: RuntimeKind::Docker,
                readiness: Readiness::Ready,
                sessions: vec![
                    Session {
                        pid: 4242,
                        command: "claude".into(),
                        agent: AgentKind::ClaudeCode,
                    },
                    Session {
                        pid: 4300,
                        command: "zsh -l".into(),
                        agent: AgentKind::Shell,
                    },
                ],
                last_seen_at: None,
            }
        })
        .collect();
    Overview {
        source: DataSource::Mock,
        runtime: RuntimeStatus::NotDetected,
        containers,
        tested_sandbox_version: "bench".into(),
        hosts: vec![],
    }
}

/// A history with `sessions` closed session records spread over 90 days and
/// 40 projects, plus the matching container records. Every fourth record
/// carries usage and a few files, like a real agent session.
pub fn history_with(sessions: usize, now: DateTime<Utc>) -> History {
    let mut history = History::default();
    let agents = [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::Pi,
        AgentKind::OpenCode,
        AgentKind::Shell,
    ];
    for i in 0..sessions {
        let started = now - Duration::minutes(((i * 7919) % (90 * 24 * 60)) as i64);
        let ended = started + Duration::minutes(5 + (i % 50) as i64);
        let project = format!("project-{:02}", i % 40);
        let container_id = format!("{:0>64x}", i / 3);
        let agent = agents[i % agents.len()];
        let usage = (i % 4 == 0).then(|| Usage {
            agent,
            input_tokens: 1000 + i as u64,
            output_tokens: 500,
            cache_read_tokens: 60_000,
            cache_write_tokens: 2_000,
            reasoning_tokens: 0,
            turns: 12,
            cost_usd: Some(0.12),
            cost_source: CostSource::Estimated,
            models: vec!["claude-sonnet-4-5-20250929".into()],
        });
        let files = if i % 4 == 0 {
            (0..3)
                .map(|f| FileTouch {
                    path: format!("src/module{f}.ts"),
                    reads: 2,
                    edits: 1,
                    created: false,
                    deleted: false,
                    last_at: Some(ended),
                })
                .collect()
        } else {
            vec![]
        };
        history.sessions.push(SessionRecord {
            id: format!("{container_id}:{}:{}", 4000 + i % 7, started.timestamp()),
            container_id: container_id.clone(),
            container_name: format!("sandbox-{project}"),
            project_slug: project.clone(),
            runtime: RuntimeKind::Docker,
            agent,
            command: "claude".into(),
            pid: 4000 + (i % 7) as u32,
            started_at: started,
            ended_at: Some(ended),
            last_seen_at: ended,
            usage,
            files,
            usage_scope: UsageScope::Session,
        });
        if i % 3 == 0 {
            history.containers.push(ContainerRecord {
                container_id,
                name: format!("sandbox-{project}"),
                project_slug: project,
                runtime: RuntimeKind::Docker,
                started_at: started,
                ended_at: Some(ended),
                last_seen_at: ended,
            });
        }
    }
    history
}

pub fn everything(now: DateTime<Utc>) -> HistoryQuery {
    HistoryQuery {
        since: now - Duration::days(365),
        project: None,
        agent: None,
    }
}

pub fn last_day_one_project(now: DateTime<Utc>) -> HistoryQuery {
    HistoryQuery {
        since: now - Duration::days(1),
        project: Some("project-03".into()),
        agent: Some(AgentKind::ClaudeCode),
    }
}
