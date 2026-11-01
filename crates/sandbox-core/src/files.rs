//! Files an agent read, created, edited or deleted, taken from the tool
//! calls in its whole transcript (not just the tail the Activity timeline
//! shows). Each agent writes tool calls differently. JSON-lines transcripts
//! are fed line by line by `transcripts`; OpenCode's message store is dumped
//! by [`opencode_files_command`].

use std::collections::HashMap;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::AgentKind;

/// `$0` of the extraction script, filtered from the event stream by the
/// shared `sandbox-watcher-` prefix.
pub const FILES_SENTINEL: &str = "sandbox-watcher-files";

/// Upper bound for one OpenCode dump, so a runaway store cannot stall a
/// refresh. Output of that size or more is reported as incomplete.
pub const OPENCODE_DUMP_CAP: usize = 16 * 1024 * 1024;
/// Files kept per agent session, changed ones first.
pub const MAX_FILES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileOp {
    Read,
    Create,
    Edit,
    Delete,
}

/// One tool call that touched a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    pub path: String,
    pub op: FileOp,
    pub at: Option<DateTime<Utc>>,
}

/// Everything an agent did to one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    pub reads: u32,
    pub edits: u32,
    pub created: bool,
    pub deleted: bool,
    pub last_at: Option<DateTime<Utc>>,
}

impl FileTouch {
    pub fn changed(&self) -> bool {
        self.edits > 0 || self.created || self.deleted
    }
}

fn sq(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Script printing OpenCode's tool parts of one session as `P:<json>` lines.
pub fn opencode_files_command(session_dir: &str) -> Vec<String> {
    let src = sq(session_dir);
    let script = format!(
        r#"d={src}; root=$(dirname "$(dirname "$d")"); for m in $(ls "$d" 2>/dev/null); do id=${{m%.json}}; for p in "$root"/part/"$id"/*.json; do [ -f "$p" ] || continue; c=$(tr -d '\n' < "$p"); case "$c" in *'"tool"'*) printf 'P:%s\n' "$c";; esac; done; done | head -c {OPENCODE_DUMP_CAP}; exit 0"#
    );
    vec!["sh".into(), "-c".into(), script, FILES_SENTINEL.into()]
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn line_time(v: &Value) -> Option<DateTime<Utc>> {
    v.get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(Into::into)
}

fn push(out: &mut Vec<FileEvent>, path: Option<&str>, op: FileOp, at: Option<DateTime<Utc>>) {
    if let Some(path) = path {
        out.push(FileEvent {
            path: path.to_string(),
            op,
            at,
        });
    }
}

/// Claude Code: `tool_use` items with `file_path` (or `notebook_path`).
pub fn parse_claude(text: &str) -> Vec<FileEvent> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = line_time(&v);
        let Some(items) = v.pointer("/message/content").and_then(Value::as_array) else {
            continue;
        };
        for item in items
            .iter()
            .filter(|i| i.get("type").and_then(Value::as_str) == Some("tool_use"))
        {
            let input = item.get("input").unwrap_or(&Value::Null);
            match item.get("name").and_then(Value::as_str).unwrap_or("") {
                "Read" => push(&mut out, str_of(input, "file_path"), FileOp::Read, at),
                "Write" => push(&mut out, str_of(input, "file_path"), FileOp::Create, at),
                "Edit" | "MultiEdit" => {
                    push(&mut out, str_of(input, "file_path"), FileOp::Edit, at)
                }
                "NotebookEdit" => push(&mut out, str_of(input, "notebook_path"), FileOp::Edit, at),
                _ => {}
            }
        }
    }
    out
}

/// Files named in an `apply_patch` body.
pub fn patch_files(patch: &str, at: Option<DateTime<Utc>>) -> Vec<FileEvent> {
    let mut out = Vec::new();
    for line in patch.lines() {
        let line = line.trim_end();
        if let Some(p) = line.strip_prefix("*** Add File: ") {
            push(&mut out, Some(p.trim()), FileOp::Create, at);
        } else if let Some(p) = line.strip_prefix("*** Update File: ") {
            push(&mut out, Some(p.trim()), FileOp::Edit, at);
        } else if let Some(p) = line.strip_prefix("*** Delete File: ") {
            push(&mut out, Some(p.trim()), FileOp::Delete, at);
        } else if let Some(p) = line.strip_prefix("*** Move to: ") {
            push(&mut out, Some(p.trim()), FileOp::Create, at);
        }
    }
    out
}

/// Reads from simple shell commands Codex uses to look at files:
/// `cat f`, `sed -n '1,80p' f`, `nl -ba f`, `head -n 40 f`, `tail f`.
pub fn shell_reads(command: &str, at: Option<DateTime<Utc>>) -> Vec<FileEvent> {
    let mut out = Vec::new();
    for part in command.split(['|', ';', '&']) {
        let words: Vec<&str> = part.split_whitespace().collect();
        let Some(program) = words.first() else {
            continue;
        };
        if !matches!(*program, "cat" | "sed" | "nl" | "head" | "tail" | "less") {
            continue;
        }
        if *program == "sed" && !words.contains(&"-n") {
            continue; // sed without -n usually rewrites, not reads
        }
        // The last argument that is not an option or a sed script.
        if let Some(last) = words.iter().skip(1).rev().find(|w| {
            !w.starts_with('-')
                && !w.starts_with('\'')
                && !w.ends_with('\'')
                && !w.chars().all(|c| c.is_ascii_digit())
        }) {
            let path = last.trim_matches('"');
            if path.contains('.') || path.contains('/') {
                push(&mut out, Some(path), FileOp::Read, at);
            }
        }
    }
    out
}

fn codex_command(args: &Value) -> String {
    match args.get("command") {
        Some(Value::Array(parts)) => {
            let words: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            if words.len() >= 3
                && (words[0].ends_with("sh"))
                && (words[1] == "-lc" || words[1] == "-c")
            {
                words[2..].join(" ")
            } else {
                words.join(" ")
            }
        }
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Codex: `apply_patch` (function or custom tool) and simple read commands.
pub fn parse_codex(text: &str) -> Vec<FileEvent> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = line_time(&v);
        let item = if v.get("type").and_then(Value::as_str) == Some("response_item") {
            v.get("payload").cloned().unwrap_or(Value::Null)
        } else {
            v
        };
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        match item.get("type").and_then(Value::as_str) {
            Some("custom_tool_call") if name == "apply_patch" => {
                out.extend(patch_files(str_of(&item, "input").unwrap_or(""), at));
            }
            Some("function_call") => {
                let args: Value = str_of(&item, "arguments")
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(Value::Null);
                if name == "apply_patch" {
                    out.extend(patch_files(str_of(&args, "input").unwrap_or(""), at));
                } else {
                    let command = codex_command(&args);
                    if let Some(patch) = command.split_once("apply_patch").map(|(_, rest)| rest) {
                        out.extend(patch_files(patch, at));
                    } else {
                        out.extend(shell_reads(&command, at));
                    }
                }
            }
            Some("local_shell_call") => {
                let command = codex_command(item.get("action").unwrap_or(&Value::Null));
                out.extend(shell_reads(&command, at));
            }
            _ => {}
        }
    }
    out
}

/// Pi: `toolCall` parts named read, write or edit with `arguments.path`.
pub fn parse_pi(text: &str) -> Vec<FileEvent> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = line_time(&v);
        let Some(parts) = v.pointer("/message/content").and_then(Value::as_array) else {
            continue;
        };
        for p in parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("toolCall"))
        {
            let args = p.get("arguments").unwrap_or(&Value::Null);
            let op = match p.get("name").and_then(Value::as_str).unwrap_or("") {
                "read" => FileOp::Read,
                "write" => FileOp::Create,
                "edit" => FileOp::Edit,
                _ => continue,
            };
            push(
                &mut out,
                str_of(args, "path").or_else(|| str_of(args, "file_path")),
                op,
                at,
            );
        }
    }
    out
}

/// OpenCode: tool parts with `state.input.filePath`.
pub fn parse_opencode(text: &str) -> Vec<FileEvent> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(json) = line.strip_prefix("P:") else {
            continue;
        };
        let Ok(p) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        if p.get("type").and_then(Value::as_str) != Some("tool") {
            continue;
        }
        let state = p.get("state").unwrap_or(&Value::Null);
        let at = state
            .pointer("/time/start")
            .and_then(Value::as_i64)
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single());
        let input = state.get("input").unwrap_or(&Value::Null);
        let op = match p.get("tool").and_then(Value::as_str).unwrap_or("") {
            "read" => FileOp::Read,
            "write" => FileOp::Create,
            "edit" => FileOp::Edit,
            "patch" => {
                out.extend(patch_files(str_of(input, "patchText").unwrap_or(""), at));
                continue;
            }
            _ => continue,
        };
        push(&mut out, str_of(input, "filePath"), op, at);
    }
    out
}

pub fn parse_events(agent: AgentKind, text: &str) -> Vec<FileEvent> {
    match agent {
        AgentKind::ClaudeCode => parse_claude(text),
        AgentKind::Codex => parse_codex(text),
        AgentKind::Pi => parse_pi(text),
        AgentKind::OpenCode => parse_opencode(text),
        AgentKind::Shell | AgentKind::Other => vec![],
    }
}

/// Make a path absolute against the project directory and drop `./` noise.
pub fn resolve(path: &str, project_dir: Option<&str>) -> String {
    let path = path.trim().trim_start_matches("./");
    if path.starts_with('/') || path.chars().nth(1) == Some(':') {
        return path.to_string();
    }
    match project_dir {
        Some(dir) => format!("{}/{}", dir.trim_end_matches('/'), path),
        None => path.to_string(),
    }
}

/// Fold events into one entry per file: changed files first, newest first.
pub fn aggregate(events: &[FileEvent], project_dir: Option<&str>) -> Vec<FileTouch> {
    let mut map: HashMap<String, FileTouch> = HashMap::new();
    for e in events {
        let path = resolve(&e.path, project_dir);
        let t = map.entry(path.clone()).or_insert_with(|| FileTouch {
            path,
            reads: 0,
            edits: 0,
            created: false,
            deleted: false,
            last_at: None,
        });
        match e.op {
            FileOp::Read => t.reads += 1,
            FileOp::Edit => t.edits += 1,
            FileOp::Create => {
                t.created = true;
                t.deleted = false;
            }
            FileOp::Delete => t.deleted = true,
        }
        if e.at.is_some() && (t.last_at.is_none() || e.at > t.last_at) {
            t.last_at = e.at;
        }
    }
    let mut list: Vec<FileTouch> = map.into_values().collect();
    list.sort_by(|a, b| {
        b.changed()
            .cmp(&a.changed())
            .then_with(|| b.last_at.cmp(&a.last_at))
            .then_with(|| a.path.cmp(&b.path))
    });
    list.truncate(MAX_FILES);
    list
}

/// Combine the touches of several transcripts of one session into one list,
/// ordered like [`aggregate`].
pub fn merge(lists: &[Vec<FileTouch>]) -> Vec<FileTouch> {
    let mut map: HashMap<String, FileTouch> = HashMap::new();
    for t in lists.iter().flatten() {
        match map.get_mut(&t.path) {
            Some(m) => {
                m.reads += t.reads;
                m.edits += t.edits;
                m.created |= t.created;
                m.deleted |= t.deleted;
                if t.last_at > m.last_at {
                    m.last_at = t.last_at;
                }
            }
            None => {
                map.insert(t.path.clone(), t.clone());
            }
        }
    }
    let mut list: Vec<FileTouch> = map.into_values().collect();
    list.sort_by(|a, b| {
        b.changed()
            .cmp(&a.changed())
            .then_with(|| b.last_at.cmp(&a.last_at))
            .then_with(|| a.path.cmp(&b.path))
    });
    list.truncate(MAX_FILES);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_tool_uses_map_to_ops() {
        let text = r#"{"type":"assistant","timestamp":"2026-10-30T10:00:00Z","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/p/src/a.ts"}},{"type":"tool_use","name":"Edit","input":{"file_path":"/p/src/a.ts","old_string":"}","new_string":"x"}}]}}
{"type":"assistant","timestamp":"2026-10-30T10:01:00Z","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"/p/src/new.ts","content":"..."}},{"type":"tool_use","name":"Bash","input":{"command":"ls"}},{"type":"tool_use","name":"NotebookEdit","input":{"notebook_path":"/p/n.ipynb"}}]}}
not json"#;
        let ops: Vec<_> = parse_claude(text)
            .into_iter()
            .map(|e| (e.path, e.op))
            .collect();
        assert_eq!(
            ops,
            vec![
                ("/p/src/a.ts".into(), FileOp::Read),
                ("/p/src/a.ts".into(), FileOp::Edit),
                ("/p/src/new.ts".into(), FileOp::Create),
                ("/p/n.ipynb".into(), FileOp::Edit),
            ]
        );
    }

    #[test]
    fn codex_patches_and_reads() {
        let text = r#"{"timestamp":"2026-10-30T10:00:00Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"sed -n '1,120p' src/cart.ts\"]}","call_id":"c1"}}
{"timestamp":"2026-10-30T10:00:05Z","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/cart.ts\n@@\n*** Add File: src/cart.test.ts\n+x\n*** Delete File: old.ts\n*** End Patch","call_id":"c2"}}
{"timestamp":"2026-10-30T10:00:09Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"cat package.json | head -n 5\"]}","call_id":"c3"}}"#;
        let ops: Vec<_> = parse_codex(text)
            .into_iter()
            .map(|e| (e.path, e.op))
            .collect();
        assert_eq!(
            ops,
            vec![
                ("src/cart.ts".into(), FileOp::Read),
                ("src/cart.ts".into(), FileOp::Edit),
                ("src/cart.test.ts".into(), FileOp::Create),
                ("old.ts".into(), FileOp::Delete),
                ("package.json".into(), FileOp::Read),
            ]
        );
    }

    #[test]
    fn shell_reads_ignore_writes_and_numbers() {
        assert!(shell_reads("sed -i 's/a/b/' x.ts", None).is_empty());
        assert!(shell_reads("ls -la", None).is_empty());
        let r = shell_reads("head -n 40 README.md", None);
        assert_eq!(r[0].path, "README.md");
    }

    #[test]
    fn pi_and_opencode_tool_calls() {
        let pi = r#"{"type":"message","timestamp":"2026-10-30T10:00:00Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"t1","name":"read","arguments":{"path":"src/a.ts"}},{"type":"toolCall","id":"t2","name":"edit","arguments":{"path":"src/a.ts","oldText":"x","newText":"y"}},{"type":"toolCall","id":"t3","name":"bash","arguments":{"command":"ls"}}]}}"#;
        assert_eq!(parse_pi(pi).len(), 2);
        let oc = r#"P:{"type":"tool","tool":"edit","callID":"a","state":{"status":"completed","input":{"filePath":"/x/src/main.ts"},"time":{"start":1761818406000}}}
P:{"type":"tool","tool":"bash","state":{"input":{"command":"ls"}}}
P:{"type":"tool","tool":"write","state":{"input":{"filePath":"/x/new.md"}}}"#;
        let ops: Vec<_> = parse_opencode(oc)
            .into_iter()
            .map(|e| (e.path, e.op))
            .collect();
        assert_eq!(
            ops,
            vec![
                ("/x/src/main.ts".into(), FileOp::Edit),
                ("/x/new.md".into(), FileOp::Create)
            ]
        );
        assert!(parse_opencode(oc)[0].at.is_some());
    }

    #[test]
    fn aggregates_per_file_changed_first() {
        let t = |s: &str| Some(DateTime::parse_from_rfc3339(s).unwrap().into());
        let events = vec![
            FileEvent {
                path: "README.md".into(),
                op: FileOp::Read,
                at: t("2026-10-30T10:05:00Z"),
            },
            FileEvent {
                path: "./src/a.ts".into(),
                op: FileOp::Read,
                at: t("2026-10-30T10:00:00Z"),
            },
            FileEvent {
                path: "/p/src/a.ts".into(),
                op: FileOp::Edit,
                at: t("2026-10-30T10:01:00Z"),
            },
            FileEvent {
                path: "src/a.ts".into(),
                op: FileOp::Edit,
                at: t("2026-10-30T10:02:00Z"),
            },
            FileEvent {
                path: "old.ts".into(),
                op: FileOp::Delete,
                at: t("2026-10-30T10:03:00Z"),
            },
        ];
        let files = aggregate(&events, Some("/p/"));
        let summary: Vec<_> = files
            .iter()
            .map(|f| (f.path.as_str(), f.reads, f.edits, f.deleted))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("/p/old.ts", 0, 0, true),
                ("/p/src/a.ts", 1, 2, false),
                ("/p/README.md", 1, 0, false),
            ]
        );
        assert_eq!(files[1].last_at, t("2026-10-30T10:02:00Z"));
    }

    #[test]
    fn resolve_keeps_absolute_and_windows_paths() {
        assert_eq!(resolve("/abs/x", Some("/p")), "/abs/x");
        assert_eq!(resolve(r"C:\w\x", Some("/p")), r"C:\w\x");
        assert_eq!(resolve("rel/x", None), "rel/x");
    }

    #[test]
    fn opencode_command_quotes_the_store_and_carries_the_sentinel() {
        let c = opencode_files_command("/home/sandbox/.local/share/opencode/storage/message/it's");
        assert_eq!(c.last().unwrap(), FILES_SENTINEL);
        assert!(c[2].contains("'/home/sandbox/.local/share/opencode/storage/message/it'\\''s'"));
        assert!(c[2].contains("part/"));
    }

    #[test]
    fn merge_adds_counts_per_path() {
        let t = |s: &str| Some(DateTime::parse_from_rfc3339(s).unwrap().into());
        let a = aggregate(
            &[FileEvent {
                path: "/p/a.ts".into(),
                op: FileOp::Read,
                at: t("2026-10-30T10:00:00Z"),
            }],
            None,
        );
        let b = aggregate(
            &[
                FileEvent {
                    path: "/p/a.ts".into(),
                    op: FileOp::Edit,
                    at: t("2026-10-30T10:05:00Z"),
                },
                FileEvent {
                    path: "/p/b.ts".into(),
                    op: FileOp::Read,
                    at: t("2026-10-30T10:01:00Z"),
                },
            ],
            None,
        );
        let merged = merge(&[a, b]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].path, "/p/a.ts");
        assert_eq!((merged[0].reads, merged[0].edits), (1, 1));
        assert_eq!(merged[0].last_at, t("2026-10-30T10:05:00Z"));
        assert_eq!(merged[1].path, "/p/b.ts");
    }
}
