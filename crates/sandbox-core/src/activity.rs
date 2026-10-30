//! What is going on inside a sandbox right now: the processes under each
//! session, the agent's own transcript when we know its format, and the
//! working-tree changes in the project.
//!
//! Everything is read through `exec` with POSIX shell only, so it works in
//! any sandbox image. Every script passes a `sandbox-watcher-*` sentinel as
//! `$0` so our own execs are filtered from the event stream.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;
use crate::adapter::{ExecRequest, RuntimeAdapter};
use crate::model::{AgentKind, MountRole, Session};
use crate::mounts::classify;
use crate::sandbox::CONTAINER_HOME;

pub const PS_SENTINEL: &str = "sandbox-watcher-ps";
pub const TRANSCRIPT_SENTINEL: &str = "sandbox-watcher-transcript";
pub const GIT_SENTINEL: &str = "sandbox-watcher-git";

/// Bytes read from the end of a transcript.
const TRANSCRIPT_TAIL_BYTES: usize = 96 * 1024;
const MAX_EVENTS: usize = 40;

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub command: String,
    pub elapsed_secs: u64,
    pub rss_bytes: u64,
    /// Depth below the session root in the tree view.
    pub depth: u32,
}

/// Script printing one header line `H|hz|uptime|pagesize` and one
/// `P|pid|ppid|starttime|rss|cmdline` line per process, straight from /proc.
pub fn ps_command() -> Vec<String> {
    let script = r#"hz=$(getconf CLK_TCK 2>/dev/null || echo 100); up=$(cut -d' ' -f1 /proc/uptime); pg=$(getconf PAGESIZE 2>/dev/null || echo 4096); echo "H|$hz|$up|$pg"; for d in /proc/[0-9]*; do p=${d#/proc/}; st=$(cat "$d/stat" 2>/dev/null) || continue; rest=${st##*)}; set -- $rest; cmd=$(tr '\0' ' ' < "$d/cmdline" 2>/dev/null); [ -n "$cmd" ] || cmd="[${st#*(}"; printf 'P|%s|%s|%s|%s|%s\n' "$p" "$2" "${20}" "${22}" "$cmd"; done"#;
    vec!["sh".into(), "-c".into(), script.into(), PS_SENTINEL.into()]
}

/// Parse [`ps_command`] output into a flat list (depth 0).
pub fn parse_ps(output: &str) -> Vec<ProcessInfo> {
    let mut hz = 100f64;
    let mut uptime = 0f64;
    let mut page = 4096u64;
    let mut out = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(6, '|');
        match parts.next() {
            Some("H") => {
                hz = parts
                    .next()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(100.0);
                uptime = parts
                    .next()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0.0);
                page = parts
                    .next()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(4096);
            }
            Some("P") => {
                let (Some(pid), Some(ppid), Some(start), Some(rss), Some(cmd)) = (
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                ) else {
                    continue;
                };
                let (Ok(pid), Ok(ppid)) = (pid.trim().parse::<u32>(), ppid.trim().parse::<u32>())
                else {
                    continue;
                };
                let start_secs = start.trim().parse::<f64>().unwrap_or(0.0) / hz;
                let rss_pages = rss.trim().parse::<u64>().unwrap_or(0);
                out.push(ProcessInfo {
                    pid,
                    ppid,
                    command: cmd.trim().trim_end_matches(']').trim_end().to_string()
                        + if cmd.trim().starts_with('[') && !cmd.trim().ends_with(']') {
                            "]"
                        } else {
                            ""
                        },
                    elapsed_secs: (uptime - start_secs).max(0.0) as u64,
                    rss_bytes: rss_pages * page,
                    depth: 0,
                });
            }
            _ => {}
        }
    }
    out
}

/// The subtree below `root_pid` in pre-order, with depths starting at 1.
pub fn descendants(all: &[ProcessInfo], root_pid: u32) -> Vec<ProcessInfo> {
    let mut children: HashMap<u32, Vec<&ProcessInfo>> = HashMap::new();
    for p in all {
        children.entry(p.ppid).or_default().push(p);
    }
    for list in children.values_mut() {
        list.sort_by_key(|p| p.pid);
    }
    let mut out = Vec::new();
    let mut stack: Vec<(u32, u32)> = vec![(root_pid, 1)];
    let mut seen = HashSet::new();
    while let Some((pid, depth)) = stack.pop() {
        if let Some(kids) = children.get(&pid) {
            for kid in kids.iter().rev() {
                if seen.insert(kid.pid) {
                    let mut info = (*kid).clone();
                    info.depth = depth;
                    out.push(info);
                    stack.push((kid.pid, depth + 1));
                }
            }
        }
    }
    // Restore pre-order (we pushed reversed children onto a stack, popping gives order).
    out
}

/// Leaves of a subtree: what is actually executing right now.
pub fn leaves(tree: &[ProcessInfo]) -> Vec<&ProcessInfo> {
    let parents: HashSet<u32> = tree.iter().map(|p| p.ppid).collect();
    tree.iter().filter(|p| !parents.contains(&p.pid)).collect()
}

// ---------------------------------------------------------------------------
// Claude Code transcript
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivityEventKind {
    UserMessage,
    AssistantText,
    ToolUse,
    ToolResult,
    ToolError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub at: Option<DateTime<Utc>>,
    pub kind: ActivityEventKind,
    /// Short label: tool name, "Claude", "You".
    pub label: String,
    /// One-line detail, truncated.
    pub detail: String,
    /// For tool use/result pairing.
    pub tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActivity {
    pub agent: AgentKind,
    /// Path of the transcript inside the container.
    pub source: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub events: Vec<ActivityEvent>,
    /// Tool call that has no result yet: what the agent is doing now.
    pub in_flight: Option<ActivityEvent>,
}

/// Lists the newest transcript per agent we know, as `agent|mtime|path`.
pub fn transcript_list_command() -> Vec<String> {
    let home = CONTAINER_HOME;
    let script = format!(
        r#"for f in $(ls -t {home}/.claude/projects/*/*.jsonl 2>/dev/null | head -1); do printf 'claude|%s|%s\n' "$(stat -c %Y "$f" 2>/dev/null || echo 0)" "$f"; done"#
    );
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

pub fn transcript_tail_command(path: &str) -> Vec<String> {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    let script = format!("tail -c {TRANSCRIPT_TAIL_BYTES} {quoted} 2>/dev/null");
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

fn truncate(text: &str, max: usize) -> String {
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let cut: String = one_line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// A compact description of a tool call's input, per well-known tool.
fn tool_detail(name: &str, input: &Value) -> String {
    let get = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    let detail = match name {
        "Bash" => get("command").to_string(),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => get("file_path").to_string(),
        "Glob" | "Grep" => {
            let pattern = get("pattern");
            let path = get("path");
            if path.is_empty() {
                pattern.to_string()
            } else {
                format!("{pattern} in {path}")
            }
        }
        "WebFetch" | "WebSearch" => {
            let u = get("url");
            if u.is_empty() {
                get("query").to_string()
            } else {
                u.to_string()
            }
        }
        "Task" | "Agent" => {
            let d = get("description");
            if d.is_empty() {
                get("prompt").to_string()
            } else {
                d.to_string()
            }
        }
        "TodoWrite" => "updating the task list".to_string(),
        _ => input
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or(&v.to_string())))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default(),
    };
    truncate(&detail, 160)
}

fn timestamp_of(value: &Value) -> Option<DateTime<Utc>> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(Into::into)
}

/// Parse the tail of a Claude Code session transcript (JSON lines). The
/// first line may be cut; it is skipped if it does not parse.
pub fn parse_claude_transcript(text: &str) -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = timestamp_of(&value);
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let content = value.pointer("/message/content");
        match kind {
            "assistant" => {
                let Some(items) = content.and_then(Value::as_array) else {
                    continue;
                };
                for item in items {
                    match item.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                            if !text.trim().is_empty() {
                                events.push(ActivityEvent {
                                    at,
                                    kind: ActivityEventKind::AssistantText,
                                    label: "Claude".into(),
                                    detail: truncate(text, 200),
                                    tool_use_id: None,
                                });
                            }
                        }
                        Some("tool_use") => {
                            let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                            events.push(ActivityEvent {
                                at,
                                kind: ActivityEventKind::ToolUse,
                                label: name.to_string(),
                                detail: tool_detail(
                                    name,
                                    item.get("input").unwrap_or(&Value::Null),
                                ),
                                tool_use_id: item
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .map(String::from),
                            });
                        }
                        _ => {}
                    }
                }
            }
            "user" => match content {
                Some(Value::String(text)) => {
                    if !text.trim().is_empty() {
                        events.push(ActivityEvent {
                            at,
                            kind: ActivityEventKind::UserMessage,
                            label: "You".into(),
                            detail: truncate(text, 200),
                            tool_use_id: None,
                        });
                    }
                }
                Some(Value::Array(items)) => {
                    for item in items {
                        match item.get("type").and_then(Value::as_str) {
                            Some("tool_result") => {
                                let is_error = item
                                    .get("is_error")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                                let body = match item.get("content") {
                                    Some(Value::String(s)) => s.clone(),
                                    Some(Value::Array(parts)) => parts
                                        .iter()
                                        .filter_map(|p| p.get("text").and_then(Value::as_str))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                    _ => String::new(),
                                };
                                events.push(ActivityEvent {
                                    at,
                                    kind: if is_error {
                                        ActivityEventKind::ToolError
                                    } else {
                                        ActivityEventKind::ToolResult
                                    },
                                    label: if is_error {
                                        "error".into()
                                    } else {
                                        "result".into()
                                    },
                                    detail: truncate(&body, 160),
                                    tool_use_id: item
                                        .get("tool_use_id")
                                        .and_then(Value::as_str)
                                        .map(String::from),
                                });
                            }
                            Some("text") => {
                                let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                                if !text.trim().is_empty() {
                                    events.push(ActivityEvent {
                                        at,
                                        kind: ActivityEventKind::UserMessage,
                                        label: "You".into(),
                                        detail: truncate(text, 200),
                                        tool_use_id: None,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    if events.len() > MAX_EVENTS {
        events.drain(..events.len() - MAX_EVENTS);
    }
    events
}

/// The last tool call without a result, if any.
pub fn in_flight(events: &[ActivityEvent]) -> Option<ActivityEvent> {
    let answered: HashSet<&str> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                ActivityEventKind::ToolResult | ActivityEventKind::ToolError
            )
        })
        .filter_map(|e| e.tool_use_id.as_deref())
        .collect();
    events
        .iter()
        .rev()
        .find(|e| {
            e.kind == ActivityEventKind::ToolUse
                && e.tool_use_id
                    .as_deref()
                    .is_some_and(|id| !answered.contains(id))
        })
        .cloned()
}

// ---------------------------------------------------------------------------
// Git working tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingTreeChange {
    /// Two-letter porcelain status, e.g. ` M`, `??`, `A `.
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingTree {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub changes: Vec<WorkingTreeChange>,
    pub truncated: bool,
    /// `git diff --shortstat` line, if any.
    pub shortstat: Option<String>,
}

const MAX_CHANGES: usize = 200;

pub fn git_status_command(project_dir: &str) -> Vec<String> {
    let quoted = format!("'{}'", project_dir.replace('\'', "'\\''"));
    let script = format!(
        "cd {quoted} 2>/dev/null || exit 3; git rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 4; \
         echo \"B|$(git rev-parse --abbrev-ref HEAD 2>/dev/null)\"; \
         git -c core.quotepath=off status --porcelain=v1 -uall 2>/dev/null | head -n {limit} | sed 's/^/S|/'; \
         echo \"D|$(git diff --shortstat 2>/dev/null)\"",
        limit = MAX_CHANGES + 1
    );
    vec!["sh".into(), "-c".into(), script, GIT_SENTINEL.into()]
}

pub fn parse_git_status(exit_code: i64, output: &str) -> WorkingTree {
    if exit_code != 0 {
        return WorkingTree::default();
    }
    let mut tree = WorkingTree {
        is_repo: true,
        ..Default::default()
    };
    for line in output.lines() {
        if let Some(branch) = line.strip_prefix("B|") {
            tree.branch = Some(branch.trim().to_string()).filter(|b| !b.is_empty());
        } else if let Some(entry) = line.strip_prefix("S|") {
            if entry.len() < 4 {
                continue;
            }
            let (status, path) = entry.split_at(2);
            tree.changes.push(WorkingTreeChange {
                status: status.to_string(),
                path: path[1..].to_string(),
            });
        } else if let Some(stat) = line.strip_prefix("D|") {
            tree.shortstat = Some(stat.trim().to_string()).filter(|s| !s.is_empty());
        }
    }
    if tree.changes.len() > MAX_CHANGES {
        tree.changes.truncate(MAX_CHANGES);
        tree.truncated = true;
    }
    tree
}

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionProcesses {
    pub session: Session,
    /// Processes below the session, pre-order with depth.
    pub tree: Vec<ProcessInfo>,
    /// Commands executing right now (leaves), newest first.
    pub running_now: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerActivity {
    pub container_id: String,
    pub collected_at: DateTime<Utc>,
    pub sessions: Vec<SessionProcesses>,
    pub agents: Vec<AgentActivity>,
    pub working_tree: Option<WorkingTree>,
    pub project_dir: Option<String>,
    pub errors: Vec<String>,
}

async fn run(
    adapter: &Arc<dyn RuntimeAdapter>,
    id: &str,
    command: Vec<String>,
) -> Result<(i64, String)> {
    let out = adapter.exec(id, ExecRequest::new(command)).await?;
    Ok((out.exit_code, out.stdout))
}

/// Project directory as seen inside the container (identity mount).
pub async fn project_container_dir(
    adapter: &Arc<dyn RuntimeAdapter>,
    id: &str,
) -> Result<Option<String>> {
    let Some(mut details) = adapter.inspect(id).await? else {
        return Ok(None);
    };
    classify(&mut details);
    Ok(details
        .mounts
        .iter()
        .find(|m| m.role == MountRole::Project)
        .map(|m| m.destination.clone()))
}

/// Gather everything for one running container.
pub async fn collect(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
    sessions: &[Session],
    now: DateTime<Utc>,
) -> ContainerActivity {
    let mut errors = Vec::new();

    let (ps_result, list_result, project_dir) = futures::join!(
        run(adapter, container_id, ps_command()),
        run(adapter, container_id, transcript_list_command()),
        project_container_dir(adapter, container_id),
    );

    let processes = match ps_result {
        Ok((0, out)) => parse_ps(&out),
        Ok((code, _)) => {
            errors.push(format!("process listing exited with {code}"));
            vec![]
        }
        Err(e) => {
            errors.push(format!("process listing failed: {e}"));
            vec![]
        }
    };
    let session_processes = sessions
        .iter()
        .map(|s| {
            let tree = descendants(&processes, s.pid);
            let mut running_now: Vec<ProcessInfo> = leaves(&tree).into_iter().cloned().collect();
            running_now.sort_by_key(|p| p.elapsed_secs);
            SessionProcesses {
                session: s.clone(),
                tree,
                running_now,
            }
        })
        .collect();

    let mut agents = Vec::new();
    if let Ok((0, listing)) = list_result {
        let mut tails = BTreeMap::new();
        for line in listing.lines() {
            let mut parts = line.splitn(3, '|');
            if let (Some("claude"), Some(mtime), Some(path)) =
                (parts.next(), parts.next(), parts.next())
            {
                let updated_at = mtime
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .and_then(|t| Utc.timestamp_opt(t, 0).single());
                tails.insert(path.trim().to_string(), updated_at);
            }
        }
        for (path, updated_at) in tails {
            match run(adapter, container_id, transcript_tail_command(&path)).await {
                Ok((0, text)) => {
                    let events = parse_claude_transcript(&text);
                    let in_flight = in_flight(&events);
                    agents.push(AgentActivity {
                        agent: AgentKind::ClaudeCode,
                        source: path,
                        updated_at,
                        events,
                        in_flight,
                    });
                }
                Ok((code, _)) => errors.push(format!("transcript read exited with {code}")),
                Err(e) => errors.push(format!("transcript read failed: {e}")),
            }
        }
    }

    let project_dir = match project_dir {
        Ok(dir) => dir,
        Err(e) => {
            errors.push(format!("inspect failed: {e}"));
            None
        }
    };
    let working_tree = match &project_dir {
        Some(dir) => match run(adapter, container_id, git_status_command(dir)).await {
            Ok((code, out)) => Some(parse_git_status(code, &out)),
            Err(e) => {
                errors.push(format!("git status failed: {e}"));
                None
            }
        },
        None => None,
    };

    ContainerActivity {
        container_id: container_id.to_string(),
        collected_at: now,
        sessions: session_processes,
        agents,
        working_tree,
        project_dir,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "H|100|5000.5|4096\n\
P|1|0|100|300|/sbin/init\n\
P|4242|1|400000|50000|claude --dangerously-skip-permissions\n\
P|4300|4242|450000|2000|/bin/sh -c npm test\n\
P|4301|4300|451000|30000|node /app/node_modules/.bin/vitest run\n\
P|4400|1|460000|1000|zsh -l\n\
P|9|2|0|0|[kworker/0:1\n";

    #[test]
    fn parses_proc_dump_and_builds_trees() {
        let all = parse_ps(PS);
        assert_eq!(all.len(), 6);
        let claude = all.iter().find(|p| p.pid == 4242).unwrap();
        assert_eq!(claude.elapsed_secs, 1000);
        assert_eq!(claude.rss_bytes, 50000 * 4096);
        assert_eq!(
            all.iter().find(|p| p.pid == 9).unwrap().command,
            "[kworker/0:1]"
        );

        let tree = descendants(&all, 4242);
        let pids: Vec<_> = tree.iter().map(|p| (p.pid, p.depth)).collect();
        assert_eq!(pids, vec![(4300, 1), (4301, 2)]);
        let now: Vec<_> = leaves(&tree).iter().map(|p| p.pid).collect();
        assert_eq!(now, vec![4301], "vitest is what runs right now");
        assert!(descendants(&all, 4400).is_empty());
    }

    const TRANSCRIPT: &str = r#"{"type":"user","message":{"role":"user","content":"please fix the failing test"},"timestamp":"2026-10-30T10:00:00Z"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Let me look at the test output first."},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"npm test -- --run","description":"Run tests"}}]},"timestamp":"2026-10-30T10:00:05Z"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"1 failed, 12 passed","is_error":true}]},"timestamp":"2026-10-30T10:00:09Z"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_2","name":"Edit","input":{"file_path":"/Users/dev/web-shop/src/cart.ts","old_string":"a","new_string":"b"}}]},"timestamp":"2026-10-30T10:00:20Z"}
{"type":"progress","timestamp":"2026-10-30T10:00:21Z"}
"#;

    #[test]
    fn parses_claude_transcript_and_finds_in_flight_call() {
        let events = parse_claude_transcript(TRANSCRIPT);
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ActivityEventKind::UserMessage,
                ActivityEventKind::AssistantText,
                ActivityEventKind::ToolUse,
                ActivityEventKind::ToolError,
                ActivityEventKind::ToolUse
            ]
        );
        assert_eq!(events[2].label, "Bash");
        assert_eq!(events[2].detail, "npm test -- --run");
        let current = in_flight(&events).unwrap();
        assert_eq!(current.label, "Edit");
        assert_eq!(current.detail, "/Users/dev/web-shop/src/cart.ts");
        // A cut first line and garbage are skipped.
        assert_eq!(
            parse_claude_transcript("ge\":{\"role\":\"user\"}}\nnot json").len(),
            0
        );
    }

    #[test]
    fn truncates_long_text_to_one_line() {
        let t = truncate("line one\n   line   two", 100);
        assert_eq!(t, "line one line two");
        assert!(truncate(&"x".repeat(300), 20).ends_with('…'));
    }

    #[test]
    fn parses_git_status() {
        let tree = parse_git_status(
            0,
            "B|feature/cart\nS| M src/cart.ts\nS|?? notes.md\nS|A  src/new.ts\nD| 2 files changed, 10 insertions(+), 2 deletions(-)\n",
        );
        assert!(tree.is_repo);
        assert_eq!(tree.branch.as_deref(), Some("feature/cart"));
        assert_eq!(tree.changes.len(), 3);
        assert_eq!(
            tree.changes[0],
            WorkingTreeChange {
                status: " M".into(),
                path: "src/cart.ts".into()
            }
        );
        assert_eq!(tree.changes[1].status, "??");
        assert!(tree.shortstat.unwrap().contains("2 files changed"));
        assert!(!parse_git_status(4, "").is_repo, "not a repository");
    }

    #[tokio::test]
    async fn collects_from_mock() {
        use crate::mock::MockAdapter;
        use crate::sessions::probe_container;
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let probe = probe_container(&adapter, "sandbox-web-shop").await.unwrap();
        let activity = collect(&adapter, "sandbox-web-shop", &probe.sessions, Utc::now()).await;
        assert!(activity.errors.is_empty(), "{:?}", activity.errors);
        assert_eq!(activity.sessions.len(), 2);
        assert_eq!(activity.sessions[0].running_now.len(), 1);
        assert_eq!(activity.agents.len(), 1);
        assert!(activity.agents[0].in_flight.is_some());
        let tree = activity.working_tree.unwrap();
        assert!(tree.is_repo);
        assert_eq!(tree.changes.len(), 2);
    }
}
