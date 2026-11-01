//! In-memory adapter for tests and for `SANDBOX_WATCHER_MOCK=1` demo mode.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use futures::StreamExt;
use futures::stream::BoxStream;
use tokio::sync::broadcast;

use crate::activity::{GIT_SENTINEL, PS_SENTINEL, TRANSCRIPT_SENTINEL};
use crate::adapter::{
    Capabilities, ContainerFilter, ExecOutput, ExecRequest, LogOptions, RuntimeAdapter,
};
use crate::files::FILES_SENTINEL;
use crate::model::{
    ContainerDetails, ContainerState, Mount, Readiness, ResourceSample, RuntimeEvent,
    RuntimeEventKind, RuntimeInfo, RuntimeKind, SandboxContainer,
};
use crate::network::NetworkSource;
use crate::sandbox::{CONTAINER_TOOLS_PATH, HASH_LABEL, PROJECT_LABEL};
use crate::transcripts::{CHUNK_BYTES, CHUNK_SENTINEL, CLOCK_SENTINEL};
use crate::turns::TURNS_SENTINEL;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct MockContainer {
    pub details: ContainerDetails,
    /// Output returned for any `exec` call.
    pub exec_output: ExecOutput,
    pub logs: String,
    /// Raw log returned for `sandbox-container-tools network diagnostic <source>`.
    pub network_logs: BTreeMap<NetworkSource, String>,
}

struct State {
    containers: Vec<MockContainer>,
    /// The Claude transcript every container serves, growable for tests.
    transcript: String,
    transcript_inode: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            containers: vec![],
            transcript: DEMO_TRANSCRIPT.into(),
            transcript_inode: 7,
        }
    }
}

/// Transcript path reported by the mock listing.
pub const DEMO_TRANSCRIPT_PATH: &str =
    "/home/sandbox/.claude/projects/-Users-dev-projects-web-shop/abc.jsonl";
/// `now` on the mock container's clock: 2026-10-30T10:00:00Z.
const DEMO_NOW: i64 = 1_793_354_400;

/// Adapter backed by a vector of containers. Clone shares the state.
#[derive(Clone)]
pub struct MockAdapter {
    kind: RuntimeKind,
    state: Arc<Mutex<State>>,
    events: broadcast::Sender<RuntimeEvent>,
    /// Never answer, like a runtime behind a stalled SSH connection.
    stalled: bool,
    /// Answer everything except `stop`.
    stall_stops: bool,
    /// Fail every `list_containers`, like a runtime whose API is broken.
    fail_lists: bool,
    /// Chunk reads served, to check that transcripts are read incrementally.
    chunk_reads: Arc<AtomicUsize>,
}

impl MockAdapter {
    pub fn empty() -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            kind: RuntimeKind::Docker,
            state: Arc::default(),
            events,
            stalled: false,
            stall_stops: false,
            fail_lists: false,
            chunk_reads: Arc::default(),
        }
    }

    pub fn with_kind(mut self, kind: RuntimeKind) -> Self {
        self.kind = kind;
        self
    }

    /// Make every call that talks to the runtime hang forever. For testing
    /// time budgets.
    pub fn stalled(mut self) -> Self {
        self.stalled = true;
        self
    }

    /// Hang only in `stop`, after the containers were listed.
    pub fn stalled_stops(mut self) -> Self {
        self.stall_stops = true;
        self
    }

    /// Fail every listing with a protocol error.
    pub fn failing_lists(mut self) -> Self {
        self.fail_lists = true;
        self
    }

    async fn stall(&self) {
        if self.stalled {
            std::future::pending::<()>().await;
        }
    }

    /// Two projects, one of them with two containers, as a realistic demo.
    pub fn demo() -> Self {
        let adapter = Self::empty();
        adapter.add(mock_container("web-shop", 1, "running", "a1b2c3d4e5f6"));
        adapter.add(mock_container("web-shop", 2, "running", "0f0f0f0f0f0f"));
        adapter.add(mock_container("billing-api", 1, "exited", "a1b2c3d4e5f6"));
        adapter.add(mock_container(
            "billing-api-live",
            1,
            "running",
            "a1b2c3d4e5f6",
        ));
        adapter
    }

    /// What the mock mode shows for the remote host `devbox`: one running
    /// container with a Claude Code session.
    pub fn remote_demo() -> Self {
        let adapter = Self::empty();
        let mut container = mock_container("ml-pipeline", 1, "running", "a1b2c3d4e5f6");
        container.details.container.id = "e5f6a7b8c9d0".into();
        adapter.add(container);
        adapter
    }

    pub fn add(&self, container: MockContainer) {
        self.state
            .lock()
            .expect("mock state poisoned")
            .containers
            .push(container);
    }

    /// Simulate a lifecycle event for subscribers of `events()`.
    pub fn emit(&self, event: RuntimeEvent) {
        let _ = self.events.send(event);
    }

    /// The agent writes one more transcript line.
    pub fn append_transcript(&self, line: &str) {
        let mut state = self.state.lock().expect("mock state poisoned");
        state.transcript.push_str(line);
        state.transcript.push('\n');
    }

    /// The transcript file is replaced by a new one with `text`.
    pub fn replace_transcript(&self, text: &str) {
        let mut state = self.state.lock().expect("mock state poisoned");
        state.transcript = format!("{text}\n");
        state.transcript_inode += 1;
    }

    /// Chunk reads served so far.
    pub fn chunk_reads(&self) -> usize {
        self.chunk_reads.load(Ordering::SeqCst)
    }

    fn find(&self, id: &str) -> Option<MockContainer> {
        let state = self.state.lock().expect("mock state poisoned");
        state
            .containers
            .iter()
            .find(|c| c.details.container.id == id || c.details.container.name == id)
            .cloned()
    }
}

/// Build a container the way sandbox would create it.
pub fn mock_container(slug: &str, instance: u32, state: &str, hash: &str) -> MockContainer {
    let name = if instance == 1 {
        format!("sandbox-{slug}")
    } else {
        format!("sandbox-{slug}-{instance}")
    };
    let id = format!("{:0>64}", format!("{slug}{instance}").replace('-', ""));
    let mut labels = BTreeMap::new();
    labels.insert(PROJECT_LABEL.to_string(), slug.to_string());
    labels.insert(HASH_LABEL.to_string(), hash.to_string());
    let container_state = ContainerState::parse(state);
    let started = Utc.with_ymd_and_hms(2026, 10, 30, 8, 0, 0).single();
    MockContainer {
        details: ContainerDetails {
            container: SandboxContainer {
                id,
                name,
                project_slug: slug.to_string(),
                image: format!("sandbox-project-{slug}:latest"),
                state: container_state,
                created_at: started,
                started_at: if container_state.is_running() {
                    started
                } else {
                    None
                },
                config_hash: Some(hash.to_string()),
                labels,
            },
            readiness: if container_state.is_running() {
                Readiness::Ready
            } else {
                Readiness::Unknown
            },
            mounts: vec![Mount {
                source: format!("/Users/dev/projects/{slug}"),
                destination: format!("/Users/dev/projects/{slug}"),
                read_only: false,
                kind: "bind".into(),
                role: Default::default(),
                host_path: None,
            }],
            ports: vec![],
            env_names: vec!["ANTHROPIC_API_KEY".into(), "TERM".into()],
            user: Some("sandbox".into()),
            working_dir: Some(format!("/Users/dev/projects/{slug}")),
            exit_code: if container_state.is_running() {
                None
            } else {
                Some(0)
            },
        },
        exec_output: ExecOutput {
            exit_code: 0,
            stdout: "ready\n4242|claude\n4300|zsh -l\n".into(),
            stderr: String::new(),
        },
        logs: "sandbox: container ready\n".into(),
        network_logs: demo_network_logs(),
    }
}

/// Small but realistic logs in sandbox's three formats.
pub fn demo_network_logs() -> BTreeMap<NetworkSource, String> {
    let mut logs = BTreeMap::new();
    logs.insert(
        NetworkSource::Firewall,
        "1761810001.1 IP 172.17.0.2.51234 > 93.184.216.34.443: Flags [S]\n\
         1761810002.2 IP 172.17.0.2.51235 > 93.184.216.34.443: Flags [S]\n"
            .into(),
    );
    logs.insert(
        NetworkSource::Dns,
        "Oct 30 08:00:05 dnsmasq[12]: reply registry.npmjs.org is 104.16.1.35\n\
         Oct 30 08:00:06 dnsmasq[12]: reply example.com is 93.184.216.34\n\
         Oct 30 08:00:07 dnsmasq[12]: config telemetry.example is NXDOMAIN\n"
            .into(),
    );
    logs.insert(
        NetworkSource::ProxyAccess,
        "1761810010.1 45 127.0.0.1 TCP_TUNNEL/200 5000 CONNECT registry.npmjs.org:443 - HIER_DIRECT/104.16.1.35 -\n\
         1761810011.4 0 127.0.0.1 TCP_DENIED/403 3456 CONNECT evil.example:443 - HIER_NONE/- text/html\n\
         1761810012.7 200 127.0.0.1 TCP_TUNNEL/200 900 CONNECT api.anthropic.com:443 - HIER_DIRECT/160.79.104.10 -\n"
            .into(),
    );
    logs
}

#[async_trait]
impl RuntimeAdapter for MockAdapter {
    fn kind(&self) -> RuntimeKind {
        self.kind
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::FULL
    }

    async fn ping(&self) -> Result<RuntimeInfo> {
        self.stall().await;
        Ok(RuntimeInfo {
            kind: self.kind,
            version: Some("mock".into()),
            endpoint: "memory".into(),
        })
    }

    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<SandboxContainer>> {
        self.stall().await;
        if self.fail_lists {
            return Err(Error::Protocol("listing failed: 500 broken".into()));
        }
        let state = self.state.lock().expect("mock state poisoned");
        Ok(state
            .containers
            .iter()
            .map(|c| c.details.container.clone())
            .filter(|c| {
                filter
                    .project_slug
                    .as_deref()
                    .is_none_or(|s| s == c.project_slug)
            })
            .filter(|c| {
                if !filter.states.is_empty() {
                    filter.states.contains(&c.state)
                } else {
                    filter.include_stopped || c.state.is_running()
                }
            })
            .collect())
    }

    async fn inspect(&self, id: &str) -> Result<Option<ContainerDetails>> {
        self.stall().await;
        Ok(self.find(id).map(|c| c.details))
    }

    async fn exec(&self, id: &str, request: ExecRequest) -> Result<ExecOutput> {
        self.stall().await;
        let container = self
            .find(id)
            .ok_or_else(|| Error::ContainerNotFound(id.into()))?;
        // `sandbox-container-tools network diagnostic <source>` serves the stored log.
        if let [tools, network, diagnostic, source] = request.command.as_slice()
            && tools == CONTAINER_TOOLS_PATH
            && network == "network"
            && diagnostic == "diagnostic"
        {
            let source = match source.as_str() {
                "firewall" => NetworkSource::Firewall,
                "dns" => NetworkSource::Dns,
                "proxy-access" => NetworkSource::ProxyAccess,
                _ => {
                    return Ok(ExecOutput {
                        exit_code: 2,
                        stdout: String::new(),
                        stderr: "unknown source".into(),
                    });
                }
            };
            let stdout = container
                .network_logs
                .get(&source)
                .cloned()
                .unwrap_or_default();
            return Ok(ExecOutput {
                exit_code: 0,
                stdout,
                stderr: String::new(),
            });
        }
        // Activity scripts are recognised by their `$0` sentinel.
        if let [sh, flag, script, sentinel, args @ ..] = request.command.as_slice()
            && sh == "sh"
            && flag == "-c"
        {
            let ok = |stdout: &str| {
                Ok(ExecOutput {
                    exit_code: 0,
                    stdout: stdout.into(),
                    stderr: String::new(),
                })
            };
            match sentinel.as_str() {
                PS_SENTINEL => {
                    return ok("H|100|5000\n\
P|1|0|100|300|/opt/sandbox-cli/entrypoint\n\
P|4242|1|400000|60000|claude --dangerously-skip-permissions\n\
P|4250|4242|480000|2000|/bin/sh -c npm test\n\
P|4251|4250|481000|40000|node node_modules/.bin/vitest run\n\
P|4300|1|420000|1500|zsh -l\n");
                }
                CLOCK_SENTINEL => {
                    let mut out = format!("C|{DEMO_NOW}|100|5000\n");
                    for pid in args {
                        match pid.as_str() {
                            "4242" => out.push_str("S|4242|400000\n"),
                            "4300" => out.push_str("S|4300|420000\n"),
                            // Started at 10:16:40, after the demo transcript's last line.
                            "4400" => out.push_str("S|4400|600000\n"),
                            _ => {}
                        }
                    }
                    return ok(&out);
                }
                CHUNK_SENTINEL => {
                    let [path, offset] = args else {
                        return ok("");
                    };
                    if path != DEMO_TRANSCRIPT_PATH {
                        return Ok(ExecOutput {
                            exit_code: 3,
                            stdout: String::new(),
                            stderr: String::new(),
                        });
                    }
                    self.chunk_reads.fetch_add(1, Ordering::SeqCst);
                    let state = self.state.lock().expect("mock state poisoned");
                    let text = &state.transcript;
                    let from = offset.parse::<usize>().unwrap_or(0).min(text.len());
                    let to = (from + CHUNK_BYTES).min(text.len());
                    return ok(&format!(
                        "S|{}|{}\n{}",
                        text.len(),
                        state.transcript_inode,
                        &text[from..to]
                    ));
                }
                TRANSCRIPT_SENTINEL if script.starts_with("tail") => {
                    let state = self.state.lock().expect("mock state poisoned");
                    return ok(&state.transcript);
                }
                TRANSCRIPT_SENTINEL => {
                    let state = self.state.lock().expect("mock state poisoned");
                    return ok(&format!(
                        "claude|{DEMO_NOW}|{}|{}|{DEMO_TRANSCRIPT_PATH}\n",
                        state.transcript.len(),
                        state.transcript_inode
                    ));
                }
                FILES_SENTINEL => {
                    return ok("");
                }
                TURNS_SENTINEL => {
                    let state = self.state.lock().expect("mock state poisoned");
                    return ok(&format!("{DEMO_EARLIER_TURN}{}", state.transcript));
                }
                GIT_SENTINEL => {
                    return ok(
                        "B|feature/cart-totals\nS| M src/cart.ts\nS|?? src/cart.test.ts\nD| 1 file changed, 14 insertions(+), 3 deletions(-)\n",
                    );
                }
                _ => {}
            }
        }
        // Honour `sh -c "exit N"` so adapter conformance tests can check exit codes.
        if let [sh, flag, script] = request.command.as_slice()
            && sh == "sh"
            && flag == "-c"
            && let Some(code) = script.trim().strip_prefix("exit ")
            && let Ok(code) = code.trim().parse::<i64>()
        {
            return Ok(ExecOutput {
                exit_code: code,
                stdout: String::new(),
                stderr: String::new(),
            });
        }
        Ok(container.exec_output)
    }

    async fn stats(&self, id: &str) -> Result<ResourceSample> {
        let c = self
            .find(id)
            .ok_or_else(|| Error::ContainerNotFound(id.into()))?;
        // Vary with time so demo sparklines move.
        let t = Utc::now().timestamp() as f64;
        let wave = (t / 7.0).sin() * 0.5 + 0.5;
        Ok(ResourceSample {
            container_id: c.details.container.id,
            at: Utc::now(),
            cpu_percent: 5.0 + wave * 60.0,
            memory_used_bytes: (400.0 + wave * 300.0) as u64 * 1024 * 1024,
            memory_limit_bytes: Some(4 * 1024 * 1024 * 1024),
            net_rx_bytes: 1_000_000,
            net_tx_bytes: 250_000,
            block_read_bytes: 0,
            block_write_bytes: 0,
        })
    }

    async fn logs(&self, id: &str, options: LogOptions) -> Result<String> {
        let c = self
            .find(id)
            .ok_or_else(|| Error::ContainerNotFound(id.into()))?;
        Ok(match options.tail {
            Some(n) => {
                let lines: Vec<&str> = c.logs.lines().collect();
                let start = lines.len().saturating_sub(n);
                lines[start..].join("\n")
            }
            None => c.logs,
        })
    }

    async fn events(&self) -> Result<BoxStream<'static, Result<RuntimeEvent>>> {
        let rx = self.events.subscribe();
        let stream = tokio_stream_from_broadcast(rx);
        Ok(stream.boxed())
    }

    async fn stop(&self, id: &str, _timeout: Option<Duration>) -> Result<()> {
        self.stall().await;
        if self.stall_stops {
            std::future::pending::<()>().await;
        }
        let mut state = self.state.lock().expect("mock state poisoned");
        let Some(c) = state
            .containers
            .iter_mut()
            .find(|c| c.details.container.id == id || c.details.container.name == id)
        else {
            return Err(Error::ContainerNotFound(id.into()));
        };
        c.details.container.state = ContainerState::Exited;
        c.details.container.started_at = None;
        let event = RuntimeEvent {
            at: Utc::now(),
            container_id: c.details.container.id.clone(),
            container_name: Some(c.details.container.name.clone()),
            kind: RuntimeEventKind::Stopped,
        };
        drop(state);
        let _ = self.events.send(event);
        Ok(())
    }
}

fn tokio_stream_from_broadcast(
    rx: broadcast::Receiver<RuntimeEvent>,
) -> impl futures::Stream<Item = Result<RuntimeEvent>> {
    // `unfold` keeps one `recv()` future alive across polls, so the waker
    // registered with the channel survives and the stream is woken on send.
    futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => return Some((Ok(event), rx)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// Usage fragments as the Claude usage grep returns them, for demo mode.
/// An earlier turn in front of [`DEMO_TRANSCRIPT`], for the turns view.
const DEMO_EARLIER_TURN: &str = r#"{"type":"user","message":{"role":"user","content":"Why does the cart show 9.99 for three items at 3.33?"},"timestamp":"2026-10-30T09:40:00Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5-20250929","id":"msg_00","role":"assistant","content":[{"type":"tool_use","id":"toolu_00","name":"Grep","input":{"pattern":"total","path":"src"}}],"usage":{"input_tokens":900,"cache_creation_input_tokens":6000,"cache_read_input_tokens":12000,"output_tokens":300}},"timestamp":"2026-10-30T09:40:06Z"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_00","content":"src/cart.ts:12: return Math.floor(sum * 100) / 100"}]},"timestamp":"2026-10-30T09:40:07Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5-20250929","id":"msg_000","role":"assistant","content":[{"type":"text","text":"total() floors instead of rounding."}],"usage":{"input_tokens":200,"cache_read_input_tokens":18000,"output_tokens":120}},"timestamp":"2026-10-30T09:41:30Z"}
"#;

/// A few lines of a Claude Code transcript for demo mode.
const DEMO_TRANSCRIPT: &str = r#"{"type":"user","message":{"role":"user","content":"Fix the cart total rounding bug and add a test"},"timestamp":"2026-10-30T09:58:00Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5-20250929","id":"msg_01","role":"assistant","usage":{"input_tokens":1200,"cache_creation_input_tokens":8000,"cache_read_input_tokens":64000,"output_tokens":900},"content":[{"type":"text","text":"I'll reproduce it with a failing test first."},{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/Users/dev/projects/web-shop/src/cart.ts"}}]},"timestamp":"2026-10-30T09:58:04Z"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"export function total(items) { ... }"}]},"timestamp":"2026-10-30T09:58:05Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5-20250929","id":"msg_02","role":"assistant","usage":{"input_tokens":300,"cache_read_input_tokens":72000,"output_tokens":1400},"content":[{"type":"tool_use","id":"toolu_02","name":"Edit","input":{"file_path":"/Users/dev/projects/web-shop/src/cart.ts","old_string":"Math.floor","new_string":"Math.round"}}]},"timestamp":"2026-10-30T09:59:10Z"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_02","content":"The file has been updated."}]},"timestamp":"2026-10-30T09:59:11Z"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Now running the suite."},{"type":"tool_use","id":"toolu_03","name":"Bash","input":{"command":"npm test -- --run","description":"Run tests"}}]},"timestamp":"2026-10-30T10:00:00Z"}
"#;

/// Docker contexts for demo mode: one importable, one that needs an alias.
pub fn demo_ssh_contexts() -> Vec<crate::endpoints::SshContext> {
    vec![
        crate::endpoints::ssh_context("staging", "ssh://dev@staging"),
        crate::endpoints::ssh_context("build-vm", "ssh://me@build-vm:2222"),
    ]
}

/// A fixed diagnostics report for the mock app, so its page stays stable.
pub fn demo_diagnostics(app_version: &str) -> crate::diagnostics::DiagnosticsReport {
    use crate::diagnostics::{
        BinaryCheck, DiagnosticsReport, EnvironmentReport, HostReport, Probe, ProbeOutcome,
    };
    use crate::remote::{HostState, HostStatus};
    let probe = |kind, candidate: &str, outcome| Probe {
        kind,
        candidate: candidate.into(),
        outcome,
    };
    DiagnosticsReport {
        generated_at: Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap(),
        app_version: app_version.into(),
        environment: EnvironmentReport {
            os: "macos".into(),
            docker_host: None,
            container_host: None,
            runtime_override: None,
            config_runtime: None,
            config_path: "/Users/me/.config/sandbox/config.toml".into(),
            binaries: vec![
                BinaryCheck {
                    name: "docker".into(),
                    found: true,
                },
                BinaryCheck {
                    name: "podman".into(),
                    found: false,
                },
                BinaryCheck {
                    name: "container".into(),
                    found: true,
                },
            ],
        },
        local: vec![
            probe(
                RuntimeKind::Docker,
                "/var/run/docker.sock",
                ProbeOutcome::Found {
                    endpoint: "unix:///var/run/docker.sock".into(),
                },
            ),
            probe(RuntimeKind::Podman, "podman on PATH", ProbeOutcome::Missing),
            probe(
                RuntimeKind::AppleContainer,
                "container on PATH",
                ProbeOutcome::Skipped {
                    reason: "pinned to docker by sandbox config".into(),
                },
            ),
        ],
        hosts: vec![HostReport {
            alias: "devbox".into(),
            status: HostStatus {
                alias: "devbox".into(),
                display_name: "devbox".into(),
                state: HostState::Connected,
                runtimes: vec![RuntimeKind::Docker],
                probes: vec!["docker: unix:///var/run/docker.sock".into()],
            },
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lists_running_by_default_and_filters_by_project() {
        let adapter = MockAdapter::demo();
        let running = adapter
            .list_containers(&ContainerFilter::running())
            .await
            .unwrap();
        assert_eq!(running.len(), 3);
        let all = adapter
            .list_containers(&ContainerFilter::all())
            .await
            .unwrap();
        assert_eq!(all.len(), 4);
        let billing = adapter
            .list_containers(&ContainerFilter::for_project("billing-api"))
            .await
            .unwrap();
        assert!(billing.is_empty(), "billing-api is exited");
    }

    #[tokio::test]
    async fn remote_demo_lists_one_running_container() {
        let adapter = MockAdapter::remote_demo();
        let all = adapter
            .list_containers(&ContainerFilter::all())
            .await
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "sandbox-ml-pipeline");
        assert_eq!(all[0].id, "e5f6a7b8c9d0");
        assert!(all[0].state.is_running());
    }

    #[tokio::test]
    async fn stop_marks_exited_and_emits_event() {
        let adapter = MockAdapter::demo();
        let mut events = adapter.events().await.unwrap();
        adapter.stop("sandbox-web-shop", None).await.unwrap();
        let details = adapter.inspect("sandbox-web-shop").await.unwrap().unwrap();
        assert_eq!(details.container.state, ContainerState::Exited);
        let event = events.next().await.unwrap().unwrap();
        assert_eq!(event.kind, RuntimeEventKind::Stopped);
    }

    #[tokio::test]
    async fn unknown_container_errors() {
        let adapter = MockAdapter::empty();
        assert!(matches!(
            adapter.stop("nope", None).await,
            Err(Error::ContainerNotFound(_))
        ));
    }
}
