//! Turn-level detail: the transcript split at every prompt the user typed,
//! with tokens, cost, tool calls and touched files per turn. Read in full on
//! request only, never on the regular refresh; the per-agent parsers of
//! `activity`, `files` and `usage` do the actual reading.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::activity::{
    ActivityEventKind, is_injected_user_text, ms_to_time, timestamp_of, transcript_events, truncate,
};
use crate::files::{FileTouch, aggregate, parse_events};
use crate::model::AgentKind;
use crate::usage::{
    PriceTable, Usage, claude_call, claude_usage_from_calls, codex_totals, codex_usage_between,
    parse_opencode_usage, parse_pi_usage,
};

/// `$0` of the turns script, filtered from the event stream by the shared
/// `sandbox-watcher-` prefix.
pub const TURNS_SENTINEL: &str = "sandbox-watcher-turns";

/// Newest part of a transcript that is read; older turns are left out.
const MAX_BYTES: usize = 32 * 1024 * 1024;
/// Newest OpenCode messages read.
const MAX_OPENCODE_MESSAGES: usize = 2000;
/// First line of the script output when the start of the transcript was cut.
const TRUNCATED_MARKER: &str = "#truncated";
/// Turns returned, newest kept.
pub const MAX_TURNS: usize = 200;

/// One prompt and everything the agent did until the next one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    /// Position in the transcript, 1 for the first prompt. 0 is activity
    /// before the first prompt that was read (a cut transcript).
    pub index: usize,
    /// What the user typed, one line, truncated.
    pub prompt: String,
    pub started_at: Option<DateTime<Utc>>,
    /// Time of the last line belonging to the turn.
    pub ended_at: Option<DateTime<Utc>>,
    pub usage: Option<Usage>,
    pub tool_calls: u32,
    pub tool_errors: u32,
    pub files: Vec<FileTouch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnReport {
    pub agent: AgentKind,
    /// Path of the transcript inside the container.
    pub source: String,
    /// Oldest first.
    pub turns: Vec<Turn>,
    /// The transcript was longer than what was read, or older turns were dropped.
    pub truncated: bool,
}

fn sq(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Script printing the whole transcript (its newest part when large), or for
/// OpenCode every message and part as `M:` and `P:` lines.
pub fn turns_command(agent: AgentKind, source: &str) -> Option<Vec<String>> {
    let src = sq(source);
    let script = match agent {
        AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::Pi => format!(
            r#"f={src}; [ -f "$f" ] || exit 0; if [ "$(wc -c < "$f")" -gt {MAX_BYTES} ]; then echo '{TRUNCATED_MARKER}'; tail -c {MAX_BYTES} "$f"; else cat "$f"; fi"#
        ),
        AgentKind::OpenCode => format!(
            r#"d={src}; root=$(dirname "$(dirname "$d")"); [ "$(ls "$d" 2>/dev/null | wc -l)" -gt {MAX_OPENCODE_MESSAGES} ] && echo '{TRUNCATED_MARKER}'; for m in $(ls "$d" 2>/dev/null | tail -n {MAX_OPENCODE_MESSAGES}); do printf 'M:%s\n' "$(tr -d '\n' < "$d/$m")"; id=${{m%.json}}; for p in "$root"/part/"$id"/*.json; do [ -f "$p" ] && printf 'P:%s\n' "$(tr -d '\n' < "$p")"; done; done | head -c {MAX_BYTES}; exit 0"#
        ),
        AgentKind::Shell | AgentKind::Other => return None,
    };
    Some(vec![
        "sh".into(),
        "-c".into(),
        script,
        TURNS_SENTINEL.into(),
    ])
}

/// Lines of one turn while splitting.
struct Chunk<'a> {
    prompt: String,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    lines: Vec<&'a str>,
    /// OpenCode: the user message whose text part becomes the prompt.
    message_id: Option<String>,
}

impl<'a> Chunk<'a> {
    fn new(prompt: String, at: Option<DateTime<Utc>>) -> Self {
        Self {
            prompt,
            started_at: at,
            ended_at: at,
            lines: vec![],
            message_id: None,
        }
    }

    fn push(&mut self, line: &'a str, at: Option<DateTime<Utc>>) {
        self.lines.push(line);
        if at.is_some() && (self.ended_at.is_none() || at > self.ended_at) {
            self.ended_at = at;
        }
        if self.started_at.is_none() {
            self.started_at = at;
        }
    }
}

fn text_parts(content: Option<&Value>, kinds: &[&str]) -> Option<String> {
    match content {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .filter(|p| {
                    p.get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|t| kinds.contains(&t))
                })
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

fn typed(text: String) -> Option<String> {
    let t = text.trim();
    (!t.is_empty() && !is_injected_user_text(t) && !t.starts_with("[Request interrupted"))
        .then(|| truncate(t, 300))
}

/// The prompt a JSONL transcript line carries, if it is one the user typed.
fn prompt_of(agent: AgentKind, v: &Value) -> Option<String> {
    match agent {
        AgentKind::ClaudeCode => {
            let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
            if v.get("type").and_then(Value::as_str) != Some("user")
                || flag("isMeta")
                || flag("isSidechain")
                || flag("isCompactSummary")
            {
                return None;
            }
            let content = v.pointer("/message/content");
            let has_result = content.and_then(Value::as_array).is_some_and(|items| {
                items
                    .iter()
                    .any(|i| i.get("type").and_then(Value::as_str) == Some("tool_result"))
            });
            if has_result {
                return None;
            }
            typed(text_parts(content, &["text"])?)
        }
        AgentKind::Codex => {
            let item = if v.get("type").and_then(Value::as_str) == Some("response_item") {
                v.get("payload")?
            } else {
                v
            };
            if item.get("type").and_then(Value::as_str) != Some("message")
                || item.get("role").and_then(Value::as_str) != Some("user")
            {
                return None;
            }
            typed(text_parts(item.get("content"), &["input_text", "text"])?)
        }
        AgentKind::Pi => {
            if v.get("type").and_then(Value::as_str) != Some("message")
                || v.pointer("/message/role").and_then(Value::as_str) != Some("user")
            {
                return None;
            }
            typed(text_parts(v.pointer("/message/content"), &["text"])?)
        }
        AgentKind::OpenCode | AgentKind::Shell | AgentKind::Other => None,
    }
}

/// Split JSON-lines transcripts at each typed prompt.
fn split_jsonl(agent: AgentKind, text: &str) -> Vec<Chunk<'_>> {
    let mut chunks = vec![Chunk::new(String::new(), None)];
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = timestamp_of(&v);
        if let Some(prompt) = prompt_of(agent, &v) {
            chunks.push(Chunk::new(prompt, at));
        }
        chunks.last_mut().expect("never empty").push(line, at);
    }
    chunks
}

/// Split the OpenCode `M:`/`P:` dump at each user message; its first text
/// part is the prompt.
fn split_opencode(text: &str) -> Vec<Chunk<'_>> {
    let mut chunks = vec![Chunk::new(String::new(), None)];
    for line in text.lines() {
        if let Some(json) = line.strip_prefix("M:") {
            let Ok(m) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            let at = ms_to_time(m.pointer("/time/created"));
            if m.get("role").and_then(Value::as_str) == Some("user") {
                let mut chunk = Chunk::new(String::new(), at);
                chunk.message_id = m.get("id").and_then(Value::as_str).map(String::from);
                chunks.push(chunk);
            }
            let done = ms_to_time(m.pointer("/time/completed")).or(at);
            chunks.last_mut().expect("never empty").push(line, done);
        } else if let Some(json) = line.strip_prefix("P:") {
            let Ok(p) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            let chunk = chunks.last_mut().expect("never empty");
            if chunk.prompt.is_empty()
                && chunk.message_id.is_some()
                && p.get("messageID").and_then(Value::as_str) == chunk.message_id.as_deref()
                && p.get("type").and_then(Value::as_str) == Some("text")
                && p.get("synthetic").and_then(Value::as_bool) != Some(true)
                && let Some(prompt) = p
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(|t| typed(t.to_string()))
            {
                chunk.prompt = prompt;
            }
            let at = ms_to_time(p.pointer("/state/time/end"))
                .or_else(|| ms_to_time(p.pointer("/time/end")));
            chunk.push(line, at);
        }
    }
    chunks
}

/// Running state for Codex, whose token counts are cumulative.
struct CodexState {
    model: String,
    /// Totals at the end of the previous turn; `None` until known.
    since: Option<Usage>,
}

fn codex_turn_usage(lines: &[&str], state: &mut CodexState, table: &PriceTable) -> Option<Usage> {
    let mut last = None;
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(model) = v
            .pointer("/payload/model")
            .and_then(Value::as_str)
            .filter(|_| {
                matches!(
                    v.get("type").and_then(Value::as_str),
                    Some("turn_context" | "session_meta")
                )
            })
        {
            state.model = model.to_string();
        }
        if let Some(totals) = codex_totals(&v) {
            last = Some(totals);
        }
    }
    let last = last?;
    let usage = codex_usage_between(state.since.as_ref(), &last, &state.model, table);
    state.since = Some(last);
    Some(usage)
}

fn turn_usage(
    agent: AgentKind,
    lines: &[&str],
    text: &str,
    codex: &mut CodexState,
    table: &PriceTable,
) -> Option<Usage> {
    let usage = match agent {
        AgentKind::ClaudeCode => {
            let calls: Vec<_> = lines
                .iter()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter_map(|v| claude_call(&v))
                .collect();
            claude_usage_from_calls(calls, table)
        }
        AgentKind::Codex => return codex_turn_usage(lines, codex, table),
        AgentKind::Pi => parse_pi_usage(text),
        AgentKind::OpenCode => {
            let messages: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("M:")).collect();
            parse_opencode_usage(&messages.join("\n"))
        }
        AgentKind::Shell | AgentKind::Other => return None,
    };
    (usage.turns > 0).then_some(usage)
}

/// Split the output of [`turns_command`] into turns.
pub fn parse_turns(
    agent: AgentKind,
    source: &str,
    output: &str,
    project_dir: Option<&str>,
    table: &PriceTable,
) -> TurnReport {
    let (mut truncated, text) = match output.strip_prefix(TRUNCATED_MARKER) {
        Some(rest) => (true, rest),
        None => (false, output),
    };
    let chunks = match agent {
        AgentKind::OpenCode => split_opencode(text),
        _ => split_jsonl(agent, text),
    };
    let mut codex = CodexState {
        model: "unknown".into(),
        // A cut transcript starts mid-session: the first counts set the baseline.
        since: (!truncated).then(|| Usage::new(AgentKind::Codex)),
    };
    let mut turns = Vec::new();
    for (index, chunk) in chunks.into_iter().enumerate() {
        let text = chunk.lines.join("\n");
        let usage = turn_usage(agent, &chunk.lines, &text, &mut codex, table);
        let events = transcript_events(agent, &text);
        let count =
            |kind: ActivityEventKind| events.iter().filter(|e| e.kind == kind).count() as u32;
        let turn = Turn {
            index,
            prompt: chunk.prompt,
            started_at: chunk.started_at,
            ended_at: chunk.ended_at,
            usage,
            tool_calls: count(ActivityEventKind::ToolUse),
            tool_errors: count(ActivityEventKind::ToolError),
            files: aggregate(&parse_events(agent, &text), project_dir),
        };
        // Lines before the first prompt only matter when something happened.
        if index == 0 && turn.usage.is_none() && turn.tool_calls == 0 && turn.files.is_empty() {
            continue;
        }
        turns.push(turn);
    }
    if turns.len() > MAX_TURNS {
        turns.drain(..turns.len() - MAX_TURNS);
        truncated = true;
    }
    TurnReport {
        agent,
        source: source.to_string(),
        turns,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{CostSource, default_prices};

    const CLAUDE: &str = r#"{"type":"user","message":{"role":"user","content":"Fix the rounding bug"},"timestamp":"2026-10-30T10:00:00Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5","id":"msg_1","content":[{"type":"thinking","thinking":""}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"output_tokens_details":{"thinking_tokens":3}}},"timestamp":"2026-10-30T10:00:02Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5","id":"msg_1","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"src/cart.ts"}}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"output_tokens_details":{"thinking_tokens":3}}},"timestamp":"2026-10-30T10:00:03Z"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"failed","is_error":true}]},"timestamp":"2026-10-30T10:00:04Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5","id":"msg_2","content":[{"type":"text","text":"Done."}],"usage":{"input_tokens":20,"output_tokens":7}},"timestamp":"2026-10-30T10:00:09Z"}
{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>x</local-command-caveat>"},"timestamp":"2026-10-30T10:01:00Z"}
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Now   add a test"}]},"timestamp":"2026-10-30T10:05:00Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-5","id":"msg_3","content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"/p/src/cart.test.ts"}},{"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"/p/src/cart.ts"}}],"usage":{"input_tokens":30,"output_tokens":40}},"timestamp":"2026-10-30T10:05:30Z"}
"#;

    #[test]
    fn claude_turns_split_at_typed_prompts() {
        let r = parse_turns(
            AgentKind::ClaudeCode,
            "/t.jsonl",
            CLAUDE,
            Some("/p"),
            &default_prices(),
        );
        assert!(!r.truncated);
        assert_eq!(r.turns.len(), 2);
        let first = &r.turns[0];
        assert_eq!(first.index, 1);
        assert_eq!(first.prompt, "Fix the rounding bug");
        assert_eq!(
            first.ended_at.unwrap().to_rfc3339(),
            "2026-10-30T10:01:00+00:00"
        );
        let u = first.usage.as_ref().unwrap();
        assert_eq!((u.input_tokens, u.output_tokens, u.turns), (30, 12, 2));
        assert_eq!(u.cost_source, CostSource::Estimated);
        assert_eq!((first.tool_calls, first.tool_errors), (1, 1));
        assert_eq!(first.files[0].path, "/p/src/cart.ts");

        let second = &r.turns[1];
        assert_eq!(
            (second.index, second.prompt.as_str()),
            (2, "Now add a test")
        );
        assert_eq!(second.tool_calls, 2);
        assert_eq!(second.usage.as_ref().unwrap().output_tokens, 40);
        assert_eq!(second.files.len(), 2);
        assert!(second.files[0].created);
    }

    #[test]
    fn cut_transcripts_keep_earlier_activity_as_turn_zero() {
        let tail = &CLAUDE[CLAUDE.find("{\"type\":\"assistant\"").unwrap() + 5..];
        let r = parse_turns(
            AgentKind::ClaudeCode,
            "/t.jsonl",
            &format!("{TRUNCATED_MARKER}\n{tail}"),
            None,
            &default_prices(),
        );
        assert!(r.truncated);
        assert_eq!(r.turns.len(), 2);
        assert_eq!((r.turns[0].index, r.turns[0].prompt.as_str()), (0, ""));
        // The cut first line is unreadable; the second line of msg_1 still counts.
        assert_eq!(r.turns[0].usage.as_ref().unwrap().turns, 2);
        assert_eq!(r.turns[1].index, 1);
    }

    #[test]
    fn codex_turns_diff_cumulative_counts() {
        let text = r#"{"timestamp":"2026-10-30T10:00:00Z","type":"session_meta","payload":{"id":"s","model":"gpt-5-codex"}}
{"timestamp":"2026-10-30T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}
{"timestamp":"2026-10-30T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"list files"}]}}
{"timestamp":"2026-10-30T10:00:03Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"cat a.txt\"]}","call_id":"c1"}}
{"timestamp":"2026-10-30T10:00:04Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":100,"reasoning_output_tokens":20}}}}
{"timestamp":"2026-10-30T10:00:05Z","type":"event_msg","payload":{"type":"token_count","info":null}}
{"timestamp":"2026-10-30T10:01:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"and again"}]}}
{"timestamp":"2026-10-30T10:01:04Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":2500,"cached_input_tokens":2000,"output_tokens":150,"reasoning_output_tokens":20}}}}
"#;
        let r = parse_turns(AgentKind::Codex, "/r.jsonl", text, None, &default_prices());
        assert_eq!(r.turns.len(), 2);
        assert_eq!(r.turns[0].prompt, "list files");
        let a = r.turns[0].usage.as_ref().unwrap();
        assert_eq!(
            (a.input_tokens, a.cache_read_tokens, a.output_tokens),
            (400, 600, 100)
        );
        assert_eq!(a.models, vec!["gpt-5-codex"]);
        assert_eq!(r.turns[0].tool_calls, 1);
        assert_eq!(r.turns[0].files[0].path, "a.txt");
        let b = r.turns[1].usage.as_ref().unwrap();
        assert_eq!(
            (b.input_tokens, b.cache_read_tokens, b.output_tokens),
            (100, 1400, 50)
        );
    }

    #[test]
    fn pi_turns_report_cost() {
        let text = r#"{"type":"message","timestamp":"2026-10-30T10:00:00Z","message":{"role":"user","content":[{"type":"text","text":"read the readme"}]}}
{"type":"message","timestamp":"2026-10-30T10:00:03Z","message":{"role":"assistant","model":"m","content":[{"type":"toolCall","id":"c1","name":"read","arguments":{"path":"README.md"}}],"usage":{"input":10,"output":2,"cost":{"total":0.01}}}}
{"type":"message","timestamp":"2026-10-30T10:00:04Z","message":{"role":"toolResult","toolCallId":"c1","isError":true,"content":[{"type":"text","text":"no such file"}]}}
"#;
        let r = parse_turns(
            AgentKind::Pi,
            "/s.jsonl",
            text,
            Some("/w"),
            &default_prices(),
        );
        assert_eq!(r.turns.len(), 1);
        let t = &r.turns[0];
        assert_eq!((t.tool_calls, t.tool_errors), (1, 1));
        assert_eq!(t.usage.as_ref().unwrap().cost_source, CostSource::Reported);
        assert_eq!(t.files[0].path, "/w/README.md");
    }

    #[test]
    fn opencode_turns_take_the_prompt_from_the_text_part() {
        let text = r#"M:{"id":"msg_1","role":"user","time":{"created":1761818400000}}
P:{"id":"p0","messageID":"msg_1","type":"text","synthetic":true,"text":"Called the read tool"}
P:{"id":"p1","messageID":"msg_1","type":"text","text":"rename the helper"}
M:{"id":"msg_2","role":"assistant","modelID":"gpt-5","time":{"created":1761818401000,"completed":1761818460000},"tokens":{"input":50,"output":5,"reasoning":0,"cache":{"read":0,"write":0}},"cost":0.002}
P:{"id":"p2","messageID":"msg_2","type":"tool","tool":"edit","callID":"c1","state":{"status":"completed","input":{"filePath":"/w/a.ts"},"output":"ok","time":{"start":1761818402000,"end":1761818403000}}}
"#;
        let r = parse_turns(AgentKind::OpenCode, "/m/ses", text, None, &default_prices());
        assert_eq!(r.turns.len(), 1);
        let t = &r.turns[0];
        assert_eq!(t.prompt, "rename the helper");
        assert_eq!(t.tool_calls, 1);
        assert_eq!(t.usage.as_ref().unwrap().cost_usd, Some(0.002));
        assert_eq!(t.files[0].path, "/w/a.ts");
        assert_eq!(t.ended_at.unwrap().timestamp(), 1761818460);
    }

    #[test]
    fn keeps_the_newest_turns() {
        let mut text = String::new();
        for i in 0..MAX_TURNS + 5 {
            text.push_str(&format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"prompt {i}\"}}}}\n"
            ));
        }
        let r = parse_turns(AgentKind::ClaudeCode, "/t", &text, None, &default_prices());
        assert_eq!(r.turns.len(), MAX_TURNS);
        assert!(r.truncated);
        assert_eq!(r.turns[0].index, 6);
        assert_eq!(
            r.turns.last().unwrap().prompt,
            format!("prompt {}", MAX_TURNS + 4)
        );
    }

    #[test]
    fn commands_carry_the_sentinel() {
        for agent in [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::Pi,
            AgentKind::OpenCode,
        ] {
            let cmd = turns_command(agent, "/x'y").unwrap();
            assert_eq!(cmd.last().unwrap(), TURNS_SENTINEL);
            assert!(cmd[2].contains(r#"'/x'\''y'"#));
        }
        assert!(turns_command(AgentKind::Shell, "/x").is_none());
    }
}
