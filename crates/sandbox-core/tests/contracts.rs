//! Serialisation contract between the Rust types and `src/types.ts`.
//!
//! Every type a Tauri command returns, accepts or emits is serialised here
//! with every enum variant and every `Option` both empty and set. The JSON
//! goes to `fixtures/contracts/<Type>.json`; `pnpm check:contracts` assigns
//! those documents to the TypeScript types and lets `tsc` reject missing
//! fields, unknown fields, wrong enum spellings and wrong nullability.
//! `fixtures/contracts/enums.json` lists every enum's wire values so the
//! TypeScript side can also be checked for variants Rust does not have.
//!
//! The test fails when a fixture differs from what the code serialises now.
//! After a deliberate change run
//! `SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-core --test contracts`,
//! then `pnpm check:contracts`, and commit the fixtures with the type change.
//! Types that live in `src-tauri` are covered by `src-tauri/src/contracts.rs`.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use sandbox_core::actions::{StopFailure, StopReport, StopResult};
use sandbox_core::activity::{
    ActivityEvent, ActivityEventKind, AgentActivity, ContainerActivity, ProcessInfo,
    SessionProcesses, WorkingTree, WorkingTreeChange,
};
use sandbox_core::adapter::Capabilities;
use sandbox_core::allowlist::{AllowResult, ConfigTarget};
use sandbox_core::detect::DetectedRuntime;
use sandbox_core::editor::EditorChoice;
use sandbox_core::endpoints::SshContext;
use sandbox_core::files::FileTouch;
use sandbox_core::history::{Aggregate, HistoryView, SessionRecord, UsageScope};
use sandbox_core::host::Host;
use sandbox_core::model::{
    AgentKind, ContainerDetails, ContainerState, Mount, MountRole, NetworkEntry, NetworkStatus,
    PortBinding, Readiness, ResourceSample, RuntimeEvent, RuntimeEventKind, RuntimeInfo,
    RuntimeKind, SandboxContainer, Session,
};
use sandbox_core::network::{NetworkReport, NetworkSource};
use sandbox_core::overview::{
    ConnectedRuntime, ContainerOverview, DataSource, Overview, RuntimeFailure, RuntimeStatus,
};
use sandbox_core::remote::{HostState, HostStatus};
use sandbox_core::settings::{NotificationSettings, RemoteHost, Settings, TerminalChoice, Theme};
use sandbox_core::transcripts::{Attribution, Coverage, Evidence};
use sandbox_core::turns::{Turn, TurnReport};
use sandbox_core::usage::{CostSource, ModelPrice, Usage};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const UPDATE_ENV: &str = "SANDBOX_WATCHER_UPDATE_CONTRACTS";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/contracts")
        .canonicalize()
        .expect("fixtures/contracts exists")
}

/// Compare `document` with the checked-in fixture, or rewrite it when
/// `SANDBOX_WATCHER_UPDATE_CONTRACTS=1`.
fn check_fixture(name: &str, document: &Value) {
    let path = fixtures_dir().join(format!("{name}.json"));
    let mut text = serde_json::to_string_pretty(document).expect("serialise fixture");
    text.push('\n');
    if std::env::var_os(UPDATE_ENV).is_some_and(|v| v == "1") {
        std::fs::write(&path, &text).expect("write fixture");
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; run with {UPDATE_ENV}=1 to create it",
            path.display()
        )
    });
    assert_eq!(
        on_disk,
        text,
        "{} is out of date; run `{UPDATE_ENV}=1 cargo test -p sandbox-core --test contracts`, \
         then `pnpm check:contracts`, and commit both",
        path.display()
    );
}

/// Serialise every sample, prove it reads back unchanged, and record it.
fn contract<T>(name: &str, samples: Vec<T>)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert!(!samples.is_empty(), "{name} needs at least one sample");
    let mut documents = Vec::with_capacity(samples.len());
    for sample in &samples {
        let value = serde_json::to_value(sample).expect("serialise");
        let back: T = serde_json::from_value(value.clone())
            .unwrap_or_else(|e| panic!("{name} does not round-trip: {e}\n{value:#}"));
        assert_eq!(&back, sample, "{name} changed in a round trip");
        documents.push(value);
    }
    check_fixture(name, &json!({ "type": name, "samples": documents }));
}

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 30, 10, 0, second).unwrap()
}

// ---------------------------------------------------------------------------
// Enum variants. Each list is checked by an exhaustive `match`, so adding a
// variant fails to compile here until it is listed.

macro_rules! unit_variants {
    ($ty:ident { $($v:ident),+ $(,)? }) => {{
        fn exhaustive(value: &$ty) {
            match value {
                $($ty::$v => {}),+
            }
        }
        let all = vec![$($ty::$v),+];
        all.iter().for_each(exhaustive);
        all
    }};
}

fn runtime_kinds() -> Vec<RuntimeKind> {
    unit_variants!(RuntimeKind {
        Docker,
        Podman,
        AppleContainer
    })
}

fn container_states() -> Vec<ContainerState> {
    unit_variants!(ContainerState {
        Created,
        Running,
        Paused,
        Stopping,
        Exited,
        Dead,
        Unknown
    })
}

fn readiness_values() -> Vec<Readiness> {
    unit_variants!(Readiness {
        Starting,
        Ready,
        Unknown
    })
}

fn agent_kinds() -> Vec<AgentKind> {
    unit_variants!(AgentKind {
        ClaudeCode,
        Codex,
        OpenCode,
        Pi,
        Shell,
        Other
    })
}

fn network_statuses() -> Vec<NetworkStatus> {
    unit_variants!(NetworkStatus { Allowed, Blocked })
}

fn network_sources() -> Vec<NetworkSource> {
    unit_variants!(NetworkSource {
        Firewall,
        Dns,
        ProxyAccess
    })
}

fn mount_roles() -> Vec<MountRole> {
    unit_variants!(MountRole {
        Project,
        Persisted,
        Runtime,
        Other
    })
}

fn data_sources() -> Vec<DataSource> {
    unit_variants!(DataSource { Mock, Live })
}

fn themes() -> Vec<Theme> {
    unit_variants!(Theme {
        System,
        Light,
        Dark
    })
}

fn activity_event_kinds() -> Vec<ActivityEventKind> {
    unit_variants!(ActivityEventKind {
        UserMessage,
        AssistantText,
        ToolUse,
        ToolResult,
        ToolError
    })
}

fn evidences() -> Vec<Evidence> {
    unit_variants!(Evidence {
        OpenFile,
        OnlySession,
        StartTime
    })
}

fn cost_sources() -> Vec<CostSource> {
    unit_variants!(CostSource {
        Reported,
        Estimated,
        Unknown
    })
}

fn usage_scopes() -> Vec<UsageScope> {
    unit_variants!(UsageScope {
        Session,
        Agent,
        Pooled
    })
}

fn config_targets() -> Vec<ConfigTarget> {
    unit_variants!(ConfigTarget { Global, Project })
}

fn runtime_event_kinds() -> Vec<RuntimeEventKind> {
    fn exhaustive(kind: &RuntimeEventKind) {
        match kind {
            RuntimeEventKind::Created
            | RuntimeEventKind::Started
            | RuntimeEventKind::Stopped
            | RuntimeEventKind::Died
            | RuntimeEventKind::ExecCreated(_)
            | RuntimeEventKind::Removed
            | RuntimeEventKind::Other(_) => {}
        }
    }
    let all = vec![
        RuntimeEventKind::Created,
        RuntimeEventKind::Started,
        RuntimeEventKind::Stopped,
        RuntimeEventKind::Died,
        RuntimeEventKind::ExecCreated("sh -c ls".into()),
        RuntimeEventKind::Removed,
        RuntimeEventKind::Other("rename".into()),
    ];
    all.iter().for_each(exhaustive);
    all
}

fn runtime_statuses() -> Vec<RuntimeStatus> {
    fn exhaustive(status: &RuntimeStatus) {
        match status {
            RuntimeStatus::NotDetected
            | RuntimeStatus::DetectedOnly { .. }
            | RuntimeStatus::Connected { .. }
            | RuntimeStatus::Unavailable { .. } => {}
        }
    }
    let all = vec![
        RuntimeStatus::NotDetected,
        RuntimeStatus::DetectedOnly {
            candidates: detected_runtimes(),
        },
        RuntimeStatus::Connected {
            runtimes: connected_runtimes(),
            failures: vec![
                RuntimeFailure {
                    host: Host::Local,
                    kind: RuntimeKind::Podman,
                    endpoint: "/run/podman/podman.sock".into(),
                    reason: "connection refused".into(),
                },
                RuntimeFailure {
                    host: Host::Remote("devbox".into()),
                    kind: RuntimeKind::Docker,
                    endpoint: "ssh://devbox".into(),
                    reason: "timed out".into(),
                },
            ],
        },
        RuntimeStatus::Unavailable {
            candidates: detected_runtimes(),
            reason: "permission denied".into(),
        },
    ];
    all.iter().for_each(exhaustive);
    all
}

fn host_states() -> Vec<HostState> {
    fn exhaustive(state: &HostState) {
        match state {
            HostState::Disabled
            | HostState::Connecting
            | HostState::Connected
            | HostState::Unreachable { .. } => {}
        }
    }
    let all = vec![
        HostState::Disabled,
        HostState::Connecting,
        HostState::Connected,
        HostState::Unreachable {
            reason: "ssh: connect to host devbox port 22: No route to host".into(),
            since: at(1),
        },
    ];
    all.iter().for_each(exhaustive);
    all
}

fn attributions() -> Vec<Attribution> {
    fn exhaustive(attribution: &Attribution) {
        match attribution {
            Attribution::Session { .. } | Attribution::Agent { .. } => {}
        }
    }
    let mut all: Vec<Attribution> = evidences()
        .into_iter()
        .map(|evidence| Attribution::Session {
            pid: 4242,
            evidence,
        })
        .collect();
    all.push(Attribution::Agent {
        candidates: vec![4242, 4300],
    });
    all.iter().for_each(exhaustive);
    all
}

fn terminal_choices() -> Vec<TerminalChoice> {
    fn exhaustive(choice: &TerminalChoice) {
        match choice {
            TerminalChoice::Auto
            | TerminalChoice::Iterm
            | TerminalChoice::TerminalApp
            | TerminalChoice::Custom(_) => {}
        }
    }
    let all = vec![
        TerminalChoice::Auto,
        TerminalChoice::Iterm,
        TerminalChoice::TerminalApp,
        TerminalChoice::Custom("wezterm start --cwd {dir}".into()),
    ];
    all.iter().for_each(exhaustive);
    all
}

fn editor_choices() -> Vec<EditorChoice> {
    fn exhaustive(choice: &EditorChoice) {
        match choice {
            EditorChoice::System
            | EditorChoice::VsCode
            | EditorChoice::Cursor
            | EditorChoice::Zed
            | EditorChoice::JetBrains
            | EditorChoice::Custom(_) => {}
        }
    }
    let all = vec![
        EditorChoice::System,
        EditorChoice::VsCode,
        EditorChoice::Cursor,
        EditorChoice::Zed,
        EditorChoice::JetBrains,
        EditorChoice::Custom("subl {path}".into()),
    ];
    all.iter().for_each(exhaustive);
    all
}

/// Wire values of every enum, for the TypeScript exhaustiveness check.
fn enum_values<T: Serialize>(values: &[T], tag: Option<&str>) -> Value {
    let values: Vec<Value> = values
        .iter()
        .map(|v| {
            let value = serde_json::to_value(v).expect("serialise");
            match tag {
                None => value,
                Some(tag) => value[tag].clone(),
            }
        })
        .collect();
    match tag {
        None => json!({ "values": values }),
        Some(tag) => json!({ "tag": tag, "values": values }),
    }
}

// ---------------------------------------------------------------------------
// Sample builders

fn detected_runtimes() -> Vec<DetectedRuntime> {
    runtime_kinds()
        .into_iter()
        .map(|kind| DetectedRuntime {
            kind,
            endpoint: format!("/var/run/{kind}.sock"),
            source: "DOCKER_HOST".into(),
        })
        .collect()
}

fn connected_runtimes() -> Vec<ConnectedRuntime> {
    vec![
        ConnectedRuntime {
            host: Host::Local,
            info: RuntimeInfo {
                kind: RuntimeKind::Docker,
                version: Some("27.3.1".into()),
                endpoint: "/var/run/docker.sock".into(),
            },
            capabilities: Capabilities {
                exec: true,
                stats: true,
                logs: true,
                events: true,
                stop: true,
            },
        },
        ConnectedRuntime {
            host: Host::Remote("devbox".into()),
            info: RuntimeInfo {
                kind: RuntimeKind::AppleContainer,
                version: None,
                endpoint: "/usr/local/bin/container".into(),
            },
            capabilities: Capabilities {
                exec: false,
                stats: false,
                logs: false,
                events: false,
                stop: false,
            },
        },
    ]
}

fn host_statuses() -> Vec<HostStatus> {
    host_states()
        .into_iter()
        .enumerate()
        .map(|(i, state)| HostStatus {
            alias: format!("host{i}"),
            display_name: format!("Host {i}"),
            state,
            runtimes: if i.is_multiple_of(2) {
                runtime_kinds()
            } else {
                vec![]
            },
            probes: if i.is_multiple_of(2) {
                vec![]
            } else {
                vec!["docker version".into()]
            },
        })
        .collect()
}

fn session(agent: AgentKind, pid: u32) -> Session {
    Session {
        pid,
        command: format!("{agent:?}").to_lowercase(),
        agent,
    }
}

fn sandbox_container(index: usize, state: ContainerState) -> SandboxContainer {
    let full = index.is_multiple_of(2);
    let mut labels = BTreeMap::new();
    labels.insert("sandbox.project".to_string(), format!("project-{index}"));
    if full {
        labels.insert("sandbox.hash".to_string(), "a1b2c3d4e5f6".to_string());
    }
    SandboxContainer {
        id: format!("{index:0>64}"),
        name: format!("sandbox-project-{index}"),
        project_slug: format!("project-{index}"),
        image: format!("sandbox-project-{index}:latest"),
        state,
        created_at: full.then(|| at(0)),
        started_at: (full && state.is_running()).then(|| at(1)),
        config_hash: full.then(|| "a1b2c3d4e5f6".to_string()),
        labels,
    }
}

fn container_overviews() -> Vec<ContainerOverview> {
    let agents = agent_kinds();
    let readiness = readiness_values();
    container_states()
        .into_iter()
        .enumerate()
        .map(|(i, state)| {
            let remote = i % 3 == 2;
            let container = sandbox_container(i, state);
            let key = if remote {
                format!("devbox/{}", container.id)
            } else {
                container.id.clone()
            };
            ContainerOverview {
                container,
                host: if remote {
                    Host::Remote("devbox".into())
                } else {
                    Host::Local
                },
                key,
                runtime: runtime_kinds()[i % 3],
                readiness: readiness[i % readiness.len()],
                sessions: if state.is_running() {
                    agents
                        .iter()
                        .enumerate()
                        .map(|(j, agent)| session(*agent, 4000 + j as u32))
                        .collect()
                } else {
                    vec![]
                },
                last_seen_at: remote.then(|| at(2)),
            }
        })
        .collect()
}

fn overviews() -> Vec<Overview> {
    let sources = data_sources();
    runtime_statuses()
        .into_iter()
        .enumerate()
        .map(|(i, runtime)| Overview {
            source: sources[i % sources.len()],
            runtime,
            containers: if i.is_multiple_of(2) {
                container_overviews()
            } else {
                vec![]
            },
            tested_sandbox_version: "0.73.1".into(),
            hosts: if i.is_multiple_of(2) {
                vec![]
            } else {
                host_statuses()
            },
        })
        .collect()
}

fn usages() -> Vec<Usage> {
    cost_sources()
        .into_iter()
        .enumerate()
        .map(|(i, cost_source)| Usage {
            agent: agent_kinds()[i % 6],
            input_tokens: 1200 * i as u64,
            output_tokens: 900,
            cache_read_tokens: 64_000,
            cache_write_tokens: 8_000,
            reasoning_tokens: 0,
            turns: 3,
            cost_usd: (cost_source != CostSource::Unknown).then_some(0.0425),
            cost_source,
            models: if i == 0 {
                vec![]
            } else {
                vec!["claude-sonnet-4-5-20250929".into()]
            },
        })
        .collect()
}

fn file_touches() -> Vec<FileTouch> {
    vec![
        FileTouch {
            path: "src/cart.ts".into(),
            reads: 2,
            edits: 1,
            created: false,
            deleted: false,
            last_at: Some(at(3)),
        },
        FileTouch {
            path: "src/cart.test.ts".into(),
            reads: 0,
            edits: 0,
            created: true,
            deleted: true,
            last_at: None,
        },
    ]
}

fn coverages() -> Vec<Coverage> {
    vec![
        Coverage {
            complete: true,
            pending_bytes: 0,
            skipped_bytes: 0,
            restarts: 0,
        },
        Coverage {
            complete: false,
            pending_bytes: 4_194_304,
            skipped_bytes: 12,
            restarts: 1,
        },
    ]
}

fn activity_events() -> Vec<ActivityEvent> {
    activity_event_kinds()
        .into_iter()
        .enumerate()
        .map(|(i, kind)| ActivityEvent {
            at: (i.is_multiple_of(2)).then(|| at(i as u32)),
            kind,
            label: "Read".into(),
            detail: "/Users/dev/projects/web-shop/src/cart.ts".into(),
            tool_use_id: (!i.is_multiple_of(2)).then(|| format!("toolu_{i:02}")),
        })
        .collect()
}

fn process_infos() -> Vec<ProcessInfo> {
    vec![
        ProcessInfo {
            pid: 4242,
            ppid: 1,
            command: "claude --dangerously-skip-permissions".into(),
            elapsed_secs: 4000,
            rss_bytes: 60_000 * 1024,
            depth: 0,
        },
        ProcessInfo {
            pid: 4251,
            ppid: 4242,
            command: "node node_modules/.bin/vitest run".into(),
            elapsed_secs: 10,
            rss_bytes: 40_000 * 1024,
            depth: 1,
        },
    ]
}

fn agent_activities() -> Vec<AgentActivity> {
    let usages = usages();
    let coverages = coverages();
    attributions()
        .into_iter()
        .enumerate()
        .map(|(i, attribution)| {
            let full = i.is_multiple_of(2);
            AgentActivity {
                agent: agent_kinds()[i % 4],
                source: "/home/sandbox/.claude/projects/-Users-dev-projects-web-shop/abc.jsonl"
                    .into(),
                updated_at: full.then(|| at(5)),
                events: if full { activity_events() } else { vec![] },
                in_flight: full.then(|| activity_events()[2].clone()),
                usage: full.then(|| usages[i % usages.len()].clone()),
                session_usage: (!full).then(|| usages[i % usages.len()].clone()),
                files: if full { file_touches() } else { vec![] },
                attribution,
                coverage: coverages[i % coverages.len()],
            }
        })
        .collect()
}

fn working_trees() -> Vec<WorkingTree> {
    vec![
        WorkingTree {
            is_repo: true,
            branch: Some("feature/cart-totals".into()),
            changes: vec![
                WorkingTreeChange {
                    status: " M".into(),
                    path: "src/cart.ts".into(),
                },
                WorkingTreeChange {
                    status: "??".into(),
                    path: "src/cart.test.ts".into(),
                },
            ],
            truncated: true,
            shortstat: Some(" 1 file changed, 14 insertions(+), 3 deletions(-)".into()),
        },
        WorkingTree {
            is_repo: true,
            branch: None,
            changes: vec![],
            truncated: false,
            shortstat: None,
        },
    ]
}

fn container_activities() -> Vec<ContainerActivity> {
    let trees = working_trees();
    (0..3)
        .map(|i: usize| {
            let full = i.is_multiple_of(2);
            ContainerActivity {
                container_id: format!("{i:0>64}"),
                collected_at: at(10),
                sessions: agent_kinds()
                    .into_iter()
                    .enumerate()
                    .map(|(j, agent)| SessionProcesses {
                        session: session(agent, 4000 + j as u32),
                        started_at: (j.is_multiple_of(2)).then(|| at(j as u32)),
                        tree: if full { process_infos() } else { vec![] },
                        running_now: if full {
                            process_infos()[1..].to_vec()
                        } else {
                            vec![]
                        },
                    })
                    .collect(),
                agents: if full { agent_activities() } else { vec![] },
                working_tree: (i < 2).then(|| trees[i].clone()),
                project_dir: full.then(|| "/Users/dev/projects/web-shop".to_string()),
                errors: if full {
                    vec![]
                } else {
                    vec!["process listing exited with 1".into()]
                },
            }
        })
        .collect()
}

fn turn_reports() -> Vec<TurnReport> {
    let usages = usages();
    vec![
        TurnReport {
            agent: AgentKind::ClaudeCode,
            source: "/home/sandbox/.claude/projects/-Users-dev-projects-web-shop/abc.jsonl".into(),
            turns: (0..3)
                .map(|i: usize| {
                    let full = i.is_multiple_of(2);
                    Turn {
                        index: i,
                        prompt: "Fix the cart total rounding bug".into(),
                        started_at: full.then(|| at(i as u32)),
                        ended_at: full.then(|| at(i as u32 + 1)),
                        usage: full.then(|| usages[i % usages.len()].clone()),
                        tool_calls: 4,
                        tool_errors: 1,
                        files: if full { file_touches() } else { vec![] },
                    }
                })
                .collect(),
            truncated: true,
        },
        TurnReport {
            agent: AgentKind::Codex,
            source: "/home/sandbox/.codex/sessions/2026/10/30/rollout-1.jsonl".into(),
            turns: vec![],
            truncated: false,
        },
    ]
}

fn container_details() -> Vec<ContainerDetails> {
    let roles = mount_roles();
    vec![
        ContainerDetails {
            container: sandbox_container(0, ContainerState::Running),
            readiness: Readiness::Ready,
            mounts: roles
                .iter()
                .enumerate()
                .map(|(i, role)| Mount {
                    source: format!("/Users/dev/projects/web-shop/{i}"),
                    destination: format!("/Users/dev/projects/web-shop/{i}"),
                    read_only: !i.is_multiple_of(2),
                    kind: if i.is_multiple_of(2) {
                        "bind".into()
                    } else {
                        "volume".into()
                    },
                    role: *role,
                    host_path: (i.is_multiple_of(2))
                        .then(|| format!("C:\\Users\\dev\\projects\\{i}")),
                })
                .collect(),
            ports: vec![
                PortBinding {
                    container_port: 3000,
                    host_port: Some(3000),
                    protocol: "tcp".into(),
                },
                PortBinding {
                    container_port: 5353,
                    host_port: None,
                    protocol: "udp".into(),
                },
            ],
            env_names: vec!["ANTHROPIC_API_KEY".into(), "TERM".into()],
            user: Some("sandbox".into()),
            working_dir: Some("/Users/dev/projects/web-shop".into()),
            exit_code: None,
        },
        ContainerDetails {
            container: sandbox_container(1, ContainerState::Exited),
            readiness: Readiness::Unknown,
            mounts: vec![],
            ports: vec![],
            env_names: vec![],
            user: None,
            working_dir: None,
            exit_code: Some(137),
        },
    ]
}

fn network_reports() -> Vec<NetworkReport> {
    let statuses = network_statuses();
    vec![
        NetworkReport {
            container_id: "0".repeat(64),
            collected_at: at(20),
            entries: statuses
                .iter()
                .enumerate()
                .map(|(i, status)| NetworkEntry {
                    container_id: "0".repeat(64),
                    destination: "api.anthropic.com".into(),
                    dst_ip: (i.is_multiple_of(2)).then(|| "93.184.216.34".to_string()),
                    port: if i.is_multiple_of(2) { 443 } else { 0 },
                    count: 12,
                    last_seen: (i.is_multiple_of(2)).then(|| at(19)),
                    status: *status,
                })
                .collect(),
            unparsed_sources: network_sources(),
            failed_sources: vec![],
        },
        NetworkReport {
            container_id: format!("{}1", "0".repeat(63)),
            collected_at: at(21),
            entries: vec![],
            unparsed_sources: vec![],
            failed_sources: network_sources(),
        },
    ]
}

fn resource_samples() -> Vec<ResourceSample> {
    vec![
        ResourceSample {
            container_id: "0".repeat(64),
            at: at(30),
            cpu_percent: 12.5,
            memory_used_bytes: 512 * 1024 * 1024,
            memory_limit_bytes: Some(4 * 1024 * 1024 * 1024),
            net_rx_bytes: 1_000_000,
            net_tx_bytes: 250_000,
            block_read_bytes: 0,
            block_write_bytes: 4096,
        },
        ResourceSample {
            container_id: format!("{}1", "0".repeat(63)),
            at: at(31),
            cpu_percent: 0.0,
            memory_used_bytes: 0,
            memory_limit_bytes: None,
            net_rx_bytes: 0,
            net_tx_bytes: 0,
            block_read_bytes: 0,
            block_write_bytes: 0,
        },
    ]
}

fn stop_reports() -> Vec<StopReport> {
    vec![
        StopReport {
            results: vec![
                StopResult {
                    container_id: "0".repeat(64),
                    name: "sandbox-web-shop".into(),
                    ok: true,
                    error: None,
                },
                StopResult {
                    container_id: format!("devbox/{}1", "0".repeat(63)),
                    name: "sandbox-ml-pipeline".into(),
                    ok: false,
                    error: Some("timed out after 10s".into()),
                },
            ],
            failures: vec![
                StopFailure {
                    host: Host::Local,
                    runtime: RuntimeKind::Podman,
                    error: "connection refused".into(),
                },
                StopFailure {
                    host: Host::Remote("devbox".into()),
                    runtime: RuntimeKind::Docker,
                    error: "ssh exited with 255".into(),
                },
            ],
        },
        StopReport::default(),
    ]
}

fn session_records() -> Vec<SessionRecord> {
    let usages = usages();
    usage_scopes()
        .into_iter()
        .enumerate()
        .map(|(i, usage_scope)| {
            let full = i.is_multiple_of(2);
            SessionRecord {
                id: format!("session-{i}"),
                container_id: format!("{i:0>64}"),
                container_name: format!("sandbox-project-{i}"),
                project_slug: format!("project-{i}"),
                runtime: runtime_kinds()[i % 3],
                agent: agent_kinds()[i % 6],
                command: "claude".into(),
                pid: 4242 + i as u32,
                started_at: at(40),
                ended_at: full.then(|| at(50)),
                last_seen_at: at(50),
                usage: full.then(|| usages[i % usages.len()].clone()),
                files: if full { file_touches() } else { vec![] },
                usage_scope,
            }
        })
        .collect()
}

fn aggregates() -> Vec<Aggregate> {
    vec![
        Aggregate {
            key: "claude-code".into(),
            sessions: 3,
            seconds: 1800,
            tokens: 250_000,
            cost_usd: Some(1.25),
            cost_incomplete: false,
        },
        Aggregate {
            key: "codex".into(),
            sessions: 1,
            seconds: 60,
            tokens: 0,
            cost_usd: None,
            cost_incomplete: true,
        },
    ]
}

fn history_views() -> Vec<HistoryView> {
    vec![
        HistoryView {
            sessions: session_records(),
            by_agent: aggregates(),
            by_project: aggregates(),
            total: aggregates()[0].clone(),
            projects: vec!["project-0".into(), "project-1".into()],
        },
        HistoryView {
            sessions: vec![],
            by_agent: vec![],
            by_project: vec![],
            total: aggregates()[1].clone(),
            projects: vec![],
        },
    ]
}

fn settings_samples() -> Vec<Settings> {
    let mut prices = BTreeMap::new();
    prices.insert(
        "claude-sonnet-4-5".to_string(),
        ModelPrice {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_write: 3.75,
        },
    );
    let full = Settings {
        poll_interval_secs: 5,
        runtime_override: Some(RuntimeKind::Podman),
        terminal: TerminalChoice::Custom("wezterm start --cwd {dir}".into()),
        confirm_stop: false,
        notifications: NotificationSettings {
            enabled: true,
            session_started: true,
            session_ended: false,
            container_started: true,
            container_stopped: false,
            container_died: true,
            blocked_burst: false,
            stale_container: true,
        },
        launch_at_login: true,
        close_to_tray: false,
        start_minimized: true,
        theme: themes()[2],
        blocked_burst_threshold: 10,
        blocked_burst_window_secs: 60,
        history_retention_days: 30,
        history_enabled: false,
        prices,
        check_for_updates: false,
        editor: EditorChoice::Custom("subl {path}".into()),
        remote_hosts: vec![
            RemoteHost {
                alias: "devbox".into(),
                display_name: Some("Dev box".into()),
                enabled: true,
            },
            RemoteHost {
                alias: "build-vm".into(),
                display_name: None,
                enabled: false,
            },
        ],
    };
    vec![Settings::default(), full]
}

fn runtime_events() -> Vec<RuntimeEvent> {
    runtime_event_kinds()
        .into_iter()
        .enumerate()
        .map(|(i, kind)| RuntimeEvent {
            at: at(i as u32),
            container_id: format!("{i:0>64}"),
            container_name: (i.is_multiple_of(2)).then(|| format!("sandbox-project-{i}")),
            kind,
        })
        .collect()
}

fn ssh_contexts() -> Vec<SshContext> {
    vec![
        SshContext {
            name: "staging".into(),
            endpoint: "ssh://dev@staging".into(),
            alias: Some("dev@staging".into()),
            reason: None,
            snippet: None,
        },
        SshContext {
            name: "build-vm".into(),
            endpoint: "ssh://me@build-vm:2222".into(),
            alias: None,
            reason: Some("the endpoint has a port; add an alias to ~/.ssh/config".into()),
            snippet: Some("Host build-vm\n  HostName build-vm\n  User me\n  Port 2222\n".into()),
        },
    ]
}

// ---------------------------------------------------------------------------
// Contracts: one test per TypeScript type. The names are the TypeScript names.

#[test]
fn overview() {
    contract("Overview", overviews());
}

#[test]
fn host_status() {
    contract("HostStatus", host_statuses());
}

#[test]
fn detected_runtime() {
    contract("DetectedRuntime", detected_runtimes());
}

#[test]
fn network_report() {
    contract("NetworkReport", network_reports());
}

#[test]
fn resource_sample() {
    contract("ResourceSample", resource_samples());
}

#[test]
fn stats_snapshot() {
    // `get_stats` returns `HashMap<String, Vec<ResourceSample>>`; one key
    // keeps the document deterministic.
    let mut snapshot = HashMap::new();
    snapshot.insert("0".repeat(64), resource_samples());
    contract("StatsSnapshot", vec![snapshot]);
}

#[test]
fn settings() {
    contract("Settings", settings_samples());
}

#[test]
fn terminal_choice() {
    contract("TerminalChoice", terminal_choices());
}

#[test]
fn editor_choice() {
    contract("EditorChoice", editor_choices());
}

#[test]
fn container_activity() {
    contract("ContainerActivity", container_activities());
}

#[test]
fn turn_report() {
    contract("TurnReport", turn_reports());
}

#[test]
fn details() {
    contract("ContainerDetails", container_details());
}

#[test]
fn stop_report() {
    contract("StopReport", stop_reports());
}

#[test]
fn history_view() {
    contract("HistoryView", history_views());
}

#[test]
fn usage() {
    contract("Usage", usages());
}

#[test]
fn allow_result() {
    contract(
        "AllowResult",
        vec![AllowResult {
            path: "/Users/dev/projects/web-shop/.sandbox/config.toml".into(),
            added: vec!["api.example.com:443".into()],
            already_present: vec!["registry.npmjs.org:443".into()],
        }],
    );
}

#[test]
fn config_target() {
    contract("ConfigTarget", config_targets());
}

#[test]
fn ssh_context() {
    contract("SshContext", ssh_contexts());
}

#[test]
fn runtime_event() {
    contract("RuntimeEvent", runtime_events());
}

#[test]
fn enums() {
    let mut enums = BTreeMap::new();
    enums.insert("RuntimeKind", enum_values(&runtime_kinds(), None));
    enums.insert("ContainerState", enum_values(&container_states(), None));
    enums.insert("Readiness", enum_values(&readiness_values(), None));
    enums.insert("AgentKind", enum_values(&agent_kinds(), None));
    enums.insert("NetworkStatus", enum_values(&network_statuses(), None));
    enums.insert("NetworkSource", enum_values(&network_sources(), None));
    enums.insert("MountRole", enum_values(&mount_roles(), None));
    enums.insert("DataSource", enum_values(&data_sources(), None));
    enums.insert("Theme", enum_values(&themes(), None));
    enums.insert(
        "ActivityEventKind",
        enum_values(&activity_event_kinds(), None),
    );
    enums.insert("Evidence", enum_values(&evidences(), None));
    enums.insert("CostSource", enum_values(&cost_sources(), None));
    enums.insert("UsageScope", enum_values(&usage_scopes(), None));
    enums.insert("ConfigTarget", enum_values(&config_targets(), None));
    enums.insert(
        "RuntimeEventKind",
        enum_values(&runtime_event_kinds(), Some("kind")),
    );
    enums.insert(
        "RuntimeStatus",
        enum_values(&runtime_statuses(), Some("status")),
    );
    enums.insert("HostState", enum_values(&host_states(), Some("state")));
    enums.insert("Attribution", enum_values(&attributions(), Some("kind")));
    enums.insert(
        "TerminalChoice",
        enum_values(&terminal_choices(), Some("kind")),
    );
    enums.insert("EditorChoice", enum_values(&editor_choices(), Some("kind")));
    check_fixture("enums", &serde_json::to_value(enums).unwrap());
}
