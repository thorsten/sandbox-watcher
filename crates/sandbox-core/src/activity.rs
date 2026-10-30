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
use crate::files::{FileTouch, aggregate, files_command, parse_events};
use crate::model::{AgentKind, MountRole, Session};
use crate::mounts::classify;
use crate::sandbox::CONTAINER_HOME;
use crate::usage::{PriceTable, Usage, parse_usage, usage_command};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentActivity {
    pub agent: AgentKind,
    /// Path of the transcript inside the container.
    pub source: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub events: Vec<ActivityEvent>,
    /// Tool call that has no result yet: what the agent is doing now.
    pub in_flight: Option<ActivityEvent>,
    /// Tokens and cost for the whole transcript, when the agent reports them.
    pub usage: Option<Usage>,
    /// Files read and changed over the whole transcript, changed first.
    #[serde(default)]
    pub files: Vec<FileTouch>,
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
// Transcript discovery per agent
// ---------------------------------------------------------------------------

fn agent_from_key(key: &str) -> Option<AgentKind> {
    match key {
        "claude" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "opencode" => Some(AgentKind::OpenCode),
        "pi" => Some(AgentKind::Pi),
        _ => None,
    }
}

/// Lists the newest transcript per agent as `agent|mtime|path`. For OpenCode
/// the path is the session's message directory. Locations:
/// Claude Code `~/.claude/projects/<cwd>/<session>.jsonl`,
/// Codex `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`,
/// Pi `~/.pi/agent/sessions/<cwd>/<session>.jsonl`,
/// OpenCode `~/.local/share/opencode/storage/message/<session>/<message>.json`.
pub fn transcript_list_command() -> Vec<String> {
    let home = CONTAINER_HOME;
    let script = format!(
        r#"newest() {{ ls -t "$@" 2>/dev/null | head -1; }}
f=$(newest {home}/.claude/projects/*/*.jsonl); [ -n "$f" ] && printf 'claude|%s|%s\n' "$(stat -c %Y "$f" 2>/dev/null || echo 0)" "$f"
f=$(newest {home}/.codex/sessions/*/*/*/*.jsonl {home}/.codex/sessions/*.jsonl); [ -n "$f" ] && printf 'codex|%s|%s\n' "$(stat -c %Y "$f" 2>/dev/null || echo 0)" "$f"
f=$(newest {home}/.pi/agent/sessions/*/*.jsonl); [ -n "$f" ] && printf 'pi|%s|%s\n' "$(stat -c %Y "$f" 2>/dev/null || echo 0)" "$f"
d=$(ls -td {home}/.local/share/opencode/storage/message/*/ 2>/dev/null | head -1); [ -n "$d" ] && printf 'opencode|%s|%s\n' "$(stat -c %Y "$d" 2>/dev/null || echo 0)" "${{d%/}}"
exit 0"#
    );
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

fn sq(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Reads the tail of a JSONL transcript, or dumps the last OpenCode messages
/// with their parts as `M:<json>` and `P:<json>` lines (newline-stripped).
pub fn transcript_tail_command(agent: AgentKind, source: &str) -> Vec<String> {
    let script = match agent {
        AgentKind::OpenCode => format!(
            r#"d={dir}; root=$(dirname "$(dirname "$d")"); for m in $(ls "$d" 2>/dev/null | tail -n 30); do printf 'M:%s\n' "$(tr -d '\n' < "$d/$m")"; id=${{m%.json}}; for p in "$root"/part/"$id"/*.json; do [ -f "$p" ] && printf 'P:%s\n' "$(tr -d '\n' < "$p")"; done; done; exit 0"#,
            dir = sq(source)
        ),
        _ => format!("tail -c {TRANSCRIPT_TAIL_BYTES} {} 2>/dev/null", sq(source)),
    };
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

/// Dispatch to the agent's parser.
pub fn parse_transcript(agent: AgentKind, text: &str) -> Vec<ActivityEvent> {
    match agent {
        AgentKind::ClaudeCode => parse_claude_transcript(text),
        AgentKind::Codex => parse_codex_transcript(text),
        AgentKind::Pi => parse_pi_transcript(text),
        AgentKind::OpenCode => parse_opencode_dump(text),
        AgentKind::Shell | AgentKind::Other => vec![],
    }
}

fn push_text(
    events: &mut Vec<ActivityEvent>,
    at: Option<DateTime<Utc>>,
    kind: ActivityEventKind,
    label: &str,
    text: &str,
) {
    if text.trim().is_empty() {
        return;
    }
    events.push(ActivityEvent {
        at,
        kind,
        label: label.into(),
        detail: truncate(text, 200),
        tool_use_id: None,
    });
}

fn push_result(
    events: &mut Vec<ActivityEvent>,
    at: Option<DateTime<Utc>>,
    id: Option<&str>,
    body: &str,
    is_error: bool,
) {
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
        detail: truncate(body, 160),
        tool_use_id: id.map(String::from),
    });
}

fn trim_events(events: &mut Vec<ActivityEvent>) {
    if events.len() > MAX_EVENTS {
        events.drain(..events.len() - MAX_EVENTS);
    }
}

/// Injected context (AGENTS.md, environment blocks) is not something the user typed.
fn is_injected_user_text(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('<') || t.starts_with("# AGENTS.md") || t.starts_with("<environment_context>")
}

// ---------------------------------------------------------------------------
// Codex transcript (rollout-*.jsonl)
// ---------------------------------------------------------------------------

fn command_text(value: &Value) -> String {
    match value {
        Value::Array(parts) => {
            let words: Vec<String> = parts
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect();
            // `bash -lc <cmd>` is Codex's usual wrapper: show the command itself.
            if words.len() >= 3
                && (words[0].ends_with("bash") || words[0].ends_with("sh"))
                && words[1] == "-lc"
            {
                words[2..].join(" ")
            } else {
                words.join(" ")
            }
        }
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

fn apply_patch_summary(input: &str) -> String {
    let files: Vec<&str> = input
        .lines()
        .filter_map(|l| {
            l.strip_prefix("*** Update File: ")
                .or_else(|| l.strip_prefix("*** Add File: "))
                .or_else(|| l.strip_prefix("*** Delete File: "))
        })
        .collect();
    if files.is_empty() {
        "apply patch".to_string()
    } else {
        files.join(", ")
    }
}

/// Parse Codex CLI rollout lines. Handles the envelope form
/// (`{"timestamp","type":"response_item","payload":{...}}`) and the older
/// bare item form.
pub fn parse_codex_transcript(text: &str) -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = timestamp_of(&value);
        let item = match value.get("type").and_then(Value::as_str) {
            Some("response_item") => value.get("payload").cloned().unwrap_or(Value::Null),
            Some("event_msg") | Some("session_meta") | Some("turn_context") | Some("compacted") => {
                continue;
            }
            _ => value.clone(),
        };
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("");
                let text: String = item
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter(|p| {
                                matches!(
                                    p.get("type").and_then(Value::as_str),
                                    Some("input_text" | "output_text" | "text")
                                )
                            })
                            .filter_map(|p| p.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                match role {
                    "assistant" => push_text(
                        &mut events,
                        at,
                        ActivityEventKind::AssistantText,
                        "Codex",
                        &text,
                    ),
                    "user" if !is_injected_user_text(&text) => push_text(
                        &mut events,
                        at,
                        ActivityEventKind::UserMessage,
                        "You",
                        &text,
                    ),
                    _ => {}
                }
            }
            Some("function_call") => {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                let args: Value = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(Value::Null);
                let detail = match name {
                    "shell" | "shell_command" | "exec_command" | "container.exec" => {
                        command_text(args.get("command").unwrap_or(&Value::Null))
                    }
                    "apply_patch" => {
                        apply_patch_summary(args.get("input").and_then(Value::as_str).unwrap_or(""))
                    }
                    _ => args.to_string(),
                };
                events.push(ActivityEvent {
                    at,
                    kind: ActivityEventKind::ToolUse,
                    label: name.to_string(),
                    detail: truncate(&detail, 160),
                    tool_use_id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .map(String::from),
                });
            }
            Some("custom_tool_call") => {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                let input = item.get("input").and_then(Value::as_str).unwrap_or("");
                let detail = if name == "apply_patch" {
                    apply_patch_summary(input)
                } else {
                    input.to_string()
                };
                events.push(ActivityEvent {
                    at,
                    kind: ActivityEventKind::ToolUse,
                    label: name.to_string(),
                    detail: truncate(&detail, 160),
                    tool_use_id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .map(String::from),
                });
            }
            Some("local_shell_call") => {
                let detail = command_text(item.pointer("/action/command").unwrap_or(&Value::Null));
                events.push(ActivityEvent {
                    at,
                    kind: ActivityEventKind::ToolUse,
                    label: "shell".into(),
                    detail: truncate(&detail, 160),
                    tool_use_id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .map(String::from),
                });
            }
            Some("function_call_output")
            | Some("custom_tool_call_output")
            | Some("local_shell_call_output") => {
                let raw = item.get("output").and_then(Value::as_str).unwrap_or("");
                // Output is often a JSON string with the real output and an exit code.
                let (body, is_error) = match serde_json::from_str::<Value>(raw) {
                    Ok(v) if v.is_object() => {
                        let body = v
                            .get("output")
                            .and_then(Value::as_str)
                            .unwrap_or(raw)
                            .to_string();
                        let code = v
                            .pointer("/metadata/exit_code")
                            .and_then(Value::as_i64)
                            .unwrap_or(0);
                        (body, code != 0)
                    }
                    _ => (raw.to_string(), false),
                };
                push_result(
                    &mut events,
                    at,
                    item.get("call_id").and_then(Value::as_str),
                    &body,
                    is_error,
                );
            }
            _ => {}
        }
    }
    trim_events(&mut events);
    events
}

// ---------------------------------------------------------------------------
// Pi transcript (session .jsonl)
// ---------------------------------------------------------------------------

fn pi_tool_detail(name: &str, args: &Value) -> String {
    let get = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    match name {
        "bash" => get("command").to_string(),
        "read" | "edit" | "write" => get("path").to_string(),
        "grep" | "find" => {
            let p = get("pattern");
            let path = get("path");
            if path.is_empty() {
                p.to_string()
            } else {
                format!("{p} in {path}")
            }
        }
        "ls" => get("path").to_string(),
        _ => args.to_string(),
    }
}

/// Parse Pi session lines: `{"type":"message","message":{"role":..,"content":[..]}}`
/// with `toolCall` parts and `toolResult` role messages.
pub fn parse_pi_transcript(text: &str) -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let at = timestamp_of(&value);
        let Some(message) = value.get("message") else {
            continue;
        };
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        let content = message.get("content");
        let texts = |c: Option<&Value>| -> String {
            match c {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => String::new(),
            }
        };
        match role {
            "user" => push_text(
                &mut events,
                at,
                ActivityEventKind::UserMessage,
                "You",
                &texts(content),
            ),
            "assistant" => {
                push_text(
                    &mut events,
                    at,
                    ActivityEventKind::AssistantText,
                    "Pi",
                    &texts(content),
                );
                if let Some(parts) = content.and_then(Value::as_array) {
                    for p in parts
                        .iter()
                        .filter(|p| p.get("type").and_then(Value::as_str) == Some("toolCall"))
                    {
                        let name = p.get("name").and_then(Value::as_str).unwrap_or("tool");
                        events.push(ActivityEvent {
                            at,
                            kind: ActivityEventKind::ToolUse,
                            label: name.to_string(),
                            detail: truncate(
                                &pi_tool_detail(name, p.get("arguments").unwrap_or(&Value::Null)),
                                160,
                            ),
                            tool_use_id: p.get("id").and_then(Value::as_str).map(String::from),
                        });
                    }
                }
            }
            "toolResult" => {
                let is_error = message
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                push_result(
                    &mut events,
                    at,
                    message.get("toolCallId").and_then(Value::as_str),
                    &texts(content),
                    is_error,
                );
            }
            _ => {}
        }
    }
    trim_events(&mut events);
    events
}

// ---------------------------------------------------------------------------
// OpenCode storage dump (M:/P: lines)
// ---------------------------------------------------------------------------

fn ms_to_time(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(Value::as_i64)
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
}

fn opencode_tool_detail(tool: &str, input: &Value) -> String {
    let get = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    match tool {
        "bash" => get("command").to_string(),
        "read" | "edit" | "write" | "patch" => get("filePath").to_string(),
        "glob" | "grep" => {
            let p = get("pattern");
            let path = get("path");
            if path.is_empty() {
                p.to_string()
            } else {
                format!("{p} in {path}")
            }
        }
        "webfetch" => get("url").to_string(),
        "task" => get("description").to_string(),
        "todowrite" | "todoread" => "updating the task list".to_string(),
        _ => input.to_string(),
    }
}

/// Parse the `M:`/`P:` dump produced by [`transcript_tail_command`] for OpenCode.
pub fn parse_opencode_dump(text: &str) -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    let mut roles: HashMap<String, (String, Option<DateTime<Utc>>)> = HashMap::new();
    for line in text.lines() {
        if let Some(json) = line.strip_prefix("M:") {
            let Ok(m) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            if let Some(id) = m.get("id").and_then(Value::as_str) {
                let role = m
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                roles.insert(
                    id.to_string(),
                    (role, ms_to_time(m.pointer("/time/created"))),
                );
            }
        } else if let Some(json) = line.strip_prefix("P:") {
            let Ok(p) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            let message_id = p.get("messageID").and_then(Value::as_str).unwrap_or("");
            let (role, at) = roles.get(message_id).cloned().unwrap_or_default();
            match p.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = p.get("text").and_then(Value::as_str).unwrap_or("");
                    if role == "user" {
                        push_text(&mut events, at, ActivityEventKind::UserMessage, "You", text);
                    } else {
                        push_text(
                            &mut events,
                            at,
                            ActivityEventKind::AssistantText,
                            "OpenCode",
                            text,
                        );
                    }
                }
                Some("tool") => {
                    let tool = p.get("tool").and_then(Value::as_str).unwrap_or("tool");
                    let state = p.get("state").cloned().unwrap_or(Value::Null);
                    let call_id = p.get("callID").and_then(Value::as_str).map(String::from);
                    let started = ms_to_time(state.pointer("/time/start")).or(at);
                    events.push(ActivityEvent {
                        at: started,
                        kind: ActivityEventKind::ToolUse,
                        label: tool.to_string(),
                        detail: truncate(
                            &opencode_tool_detail(tool, state.get("input").unwrap_or(&Value::Null)),
                            160,
                        ),
                        tool_use_id: call_id.clone(),
                    });
                    match state.get("status").and_then(Value::as_str) {
                        Some("completed") => {
                            let out = state.get("output").and_then(Value::as_str).unwrap_or("");
                            push_result(
                                &mut events,
                                ms_to_time(state.pointer("/time/end")).or(started),
                                call_id.as_deref(),
                                out,
                                false,
                            );
                        }
                        Some("error") => {
                            let err = state
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("error");
                            push_result(
                                &mut events,
                                ms_to_time(state.pointer("/time/end")).or(started),
                                call_id.as_deref(),
                                err,
                                true,
                            );
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    trim_events(&mut events);
    events
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Tokens and cost for one agent transcript, `None` when unreadable.
pub async fn read_usage(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
    agent: AgentKind,
    source: &str,
    prices: &PriceTable,
) -> Option<Usage> {
    let command = usage_command(agent, source)?;
    match run(adapter, container_id, command).await {
        Ok((0, out)) => parse_usage(agent, &out, prices),
        _ => None,
    }
}

/// Files an agent touched over its whole transcript.
pub async fn read_files(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
    agent: AgentKind,
    source: &str,
    project_dir: Option<&str>,
) -> Vec<FileTouch> {
    let Some(command) = files_command(agent, source) else {
        return vec![];
    };
    match run(adapter, container_id, command).await {
        Ok((0, out)) => aggregate(&parse_events(agent, &out), project_dir),
        _ => vec![],
    }
}

/// Usage and files for one agent with a running session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInsight {
    pub agent: AgentKind,
    pub usage: Option<Usage>,
    pub files: Vec<FileTouch>,
}

/// Usage and touched files for every agent with a running session. Used by
/// the watcher on every refresh to keep the history current.
pub async fn collect_insights(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
    sessions: &[Session],
    prices: &PriceTable,
) -> Vec<AgentInsight> {
    let active: HashSet<AgentKind> = sessions.iter().map(|s| s.agent).collect();
    if active.is_empty() {
        return vec![];
    }
    let Ok((0, listing)) = run(adapter, container_id, transcript_list_command()).await else {
        return vec![];
    };
    let project_dir = project_container_dir(adapter, container_id)
        .await
        .ok()
        .flatten();
    let mut out = Vec::new();
    for line in listing.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(key), Some(_), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Some(agent) = agent_from_key(key) else {
            continue;
        };
        if !active.contains(&agent) {
            continue;
        }
        let path = path.trim();
        let usage = read_usage(adapter, container_id, agent, path, prices).await;
        let files = read_files(adapter, container_id, agent, path, project_dir.as_deref()).await;
        out.push(AgentInsight {
            agent,
            usage,
            files,
        });
    }
    out
}

/// Usage for every agent with a running session. Cheap enough to run on each
/// refresh: a grep per transcript.
pub async fn collect_usage(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
    sessions: &[Session],
    prices: &PriceTable,
) -> Vec<Usage> {
    let active: HashSet<AgentKind> = sessions.iter().map(|s| s.agent).collect();
    if active.is_empty() {
        return vec![];
    }
    let Ok((0, listing)) = run(adapter, container_id, transcript_list_command()).await else {
        return vec![];
    };
    let mut out = Vec::new();
    for line in listing.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(key), Some(_), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Some(agent) = agent_from_key(key) else {
            continue;
        };
        if !active.contains(&agent) {
            continue;
        }
        if let Some(usage) = read_usage(adapter, container_id, agent, path.trim(), prices).await {
            out.push(usage);
        }
    }
    out
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
    prices: &PriceTable,
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

    // Transcripts only for agents that actually have a session here, so a
    // stale file from last week never looks like current activity.
    let project_dir_ref: Option<&str> = project_dir.as_ref().ok().and_then(|d| d.as_deref());
    let active_agents: HashSet<AgentKind> = sessions.iter().map(|s| s.agent).collect();
    let mut agents = Vec::new();
    if let Ok((0, listing)) = list_result {
        let mut found: BTreeMap<&str, (AgentKind, Option<DateTime<Utc>>, String)> = BTreeMap::new();
        for line in listing.lines() {
            let mut parts = line.splitn(3, '|');
            let (Some(key), Some(mtime), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Some(agent) = agent_from_key(key) else {
                continue;
            };
            if !active_agents.contains(&agent) {
                continue;
            }
            let updated_at = mtime
                .trim()
                .parse::<i64>()
                .ok()
                .and_then(|t| Utc.timestamp_opt(t, 0).single());
            found.insert(key, (agent, updated_at, path.trim().to_string()));
        }
        for (_, (agent, updated_at, path)) in found {
            match run(adapter, container_id, transcript_tail_command(agent, &path)).await {
                Ok((0, text)) => {
                    let events = parse_transcript(agent, &text);
                    let in_flight = in_flight(&events);
                    let usage = read_usage(adapter, container_id, agent, &path, prices).await;
                    let files =
                        read_files(adapter, container_id, agent, &path, project_dir_ref).await;
                    agents.push(AgentActivity {
                        agent,
                        source: path,
                        updated_at,
                        events,
                        in_flight,
                        usage,
                        files,
                    });
                }
                Ok((code, _)) => errors.push(format!(
                    "{} transcript read exited with {code}",
                    agent.label()
                )),
                Err(e) => errors.push(format!("{} transcript read failed: {e}", agent.label())),
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

    const CODEX: &str = r#"{"timestamp":"2026-10-30T10:00:00Z","type":"session_meta","payload":{"id":"x"}}
{"timestamp":"2026-10-30T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>cwd=/x</environment_context>"}]}}
{"timestamp":"2026-10-30T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a changelog entry"}]}}
{"timestamp":"2026-10-30T10:00:05Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"git log --oneline -5\"]}","call_id":"call_1"}}
{"timestamp":"2026-10-30T10:00:06Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_1","output":"{\"output\":\"abc fix\\n\",\"metadata\":{\"exit_code\":0}}"}}
{"timestamp":"2026-10-30T10:00:09Z","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Update File: CHANGELOG.md\n@@\n+entry\n*** End Patch","call_id":"call_2"}}
{"timestamp":"2026-10-30T10:00:10Z","type":"event_msg","payload":{"type":"agent_message","message":"Done"}}
"#;

    #[test]
    fn parses_codex_rollout() {
        let events = parse_codex_transcript(CODEX);
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ActivityEventKind::UserMessage,
                ActivityEventKind::ToolUse,
                ActivityEventKind::ToolResult,
                ActivityEventKind::ToolUse
            ],
            "{events:?}"
        );
        assert_eq!(
            events[0].detail, "add a changelog entry",
            "injected context is skipped"
        );
        assert_eq!(
            events[1].detail, "git log --oneline -5",
            "bash -lc wrapper stripped"
        );
        assert_eq!(events[3].label, "apply_patch");
        assert_eq!(events[3].detail, "CHANGELOG.md");
        assert_eq!(
            in_flight(&events).unwrap().tool_use_id.as_deref(),
            Some("call_2")
        );
    }

    const PI: &str = r#"{"type":"session","version":3,"id":"s1","timestamp":"2026-10-30T10:00:00Z","cwd":"/x"}
{"type":"message","id":"m1","timestamp":"2026-10-30T10:00:01Z","message":{"role":"user","content":[{"type":"text","text":"run the linter"}]}}
{"type":"message","id":"m2","timestamp":"2026-10-30T10:00:03Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"..."},{"type":"text","text":"Running it now."},{"type":"toolCall","id":"tc1","name":"bash","arguments":{"command":"pnpm lint"}}]}}
{"type":"message","id":"m3","timestamp":"2026-10-30T10:00:04Z","message":{"role":"toolResult","toolCallId":"tc1","toolName":"bash","content":[{"type":"text","text":"2 errors"}],"isError":true}}
{"type":"message","id":"m4","timestamp":"2026-10-30T10:00:08Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"tc2","name":"edit","arguments":{"path":"src/a.ts","oldText":"x","newText":"y"}}]}}
"#;

    #[test]
    fn parses_pi_session() {
        let events = parse_pi_transcript(PI);
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
        assert_eq!(events[2].detail, "pnpm lint");
        let current = in_flight(&events).unwrap();
        assert_eq!(
            (current.label.as_str(), current.detail.as_str()),
            ("edit", "src/a.ts")
        );
    }

    const OPENCODE: &str = r#"M:{"id":"msg_1","sessionID":"ses_1","role":"user","time":{"created":1761818400000}}
P:{"id":"prt_1","messageID":"msg_1","sessionID":"ses_1","type":"text","text":"fix the build"}
M:{"id":"msg_2","sessionID":"ses_1","role":"assistant","time":{"created":1761818405000}}
P:{"id":"prt_2","messageID":"msg_2","type":"step-start"}
P:{"id":"prt_3","messageID":"msg_2","type":"text","text":"Checking the build output."}
P:{"id":"prt_4","messageID":"msg_2","type":"tool","callID":"call_a","tool":"bash","state":{"status":"completed","input":{"command":"pnpm build"},"output":"error TS2345","time":{"start":1761818406000,"end":1761818409000}}}
P:{"id":"prt_5","messageID":"msg_2","type":"tool","callID":"call_b","tool":"edit","state":{"status":"running","input":{"filePath":"/x/src/main.ts"},"time":{"start":1761818410000}}}
"#;

    #[test]
    fn parses_opencode_storage_dump() {
        let events = parse_opencode_dump(OPENCODE);
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ActivityEventKind::UserMessage,
                ActivityEventKind::AssistantText,
                ActivityEventKind::ToolUse,
                ActivityEventKind::ToolResult,
                ActivityEventKind::ToolUse
            ]
        );
        assert_eq!(events[2].detail, "pnpm build");
        assert!(events[2].at.is_some());
        let current = in_flight(&events).unwrap();
        assert_eq!(current.label, "edit");
        assert_eq!(current.detail, "/x/src/main.ts");
    }

    #[test]
    fn transcript_commands_carry_sentinel_and_quote_paths() {
        let list = transcript_list_command();
        assert_eq!(list.last().unwrap(), TRANSCRIPT_SENTINEL);
        assert!(
            list[2].contains(".codex/sessions")
                && list[2].contains(".pi/agent/sessions")
                && list[2].contains("opencode/storage")
        );
        let tail =
            transcript_tail_command(AgentKind::Codex, "/home/sandbox/.codex/sessions/it's.jsonl");
        assert!(tail[2].contains("'/home/sandbox/.codex/sessions/it'\\''s.jsonl'"));
        let dump = transcript_tail_command(
            AgentKind::OpenCode,
            "/home/sandbox/.local/share/opencode/storage/message/ses_1",
        );
        assert!(dump[2].contains("part/"));
    }

    #[tokio::test]
    async fn collects_from_mock() {
        use crate::mock::MockAdapter;
        use crate::sessions::probe_container;
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let probe = probe_container(&adapter, "sandbox-web-shop").await.unwrap();
        let activity = collect(
            &adapter,
            "sandbox-web-shop",
            &probe.sessions,
            Utc::now(),
            &crate::usage::default_prices(),
        )
        .await;
        assert!(activity.errors.is_empty(), "{:?}", activity.errors);
        assert_eq!(activity.sessions.len(), 2);
        assert_eq!(activity.sessions[0].running_now.len(), 1);
        assert_eq!(activity.agents.len(), 1);
        assert!(activity.agents[0].in_flight.is_some());
        let usage = activity.agents[0]
            .usage
            .as_ref()
            .expect("usage from mock transcript");
        assert!(usage.input_tokens > 0 && usage.cost_usd.is_some());
        let files = &activity.agents[0].files;
        assert!(
            files
                .iter()
                .any(|f| f.path == "/Users/dev/projects/web-shop/src/cart.ts"
                    && f.edits == 1
                    && f.reads == 1),
            "{files:?}"
        );
        let tree = activity.working_tree.unwrap();
        assert!(tree.is_repo);
        assert_eq!(tree.changes.len(), 2);
    }
}
