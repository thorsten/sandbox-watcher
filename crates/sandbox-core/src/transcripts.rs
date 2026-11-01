//! Agent transcripts: finding them, reading them incrementally, and deciding
//! which running session wrote them.
//!
//! Transcripts are append-only JSON lines (OpenCode: a directory of JSON
//! files). Each file is read in chunks from the byte after the last complete
//! line seen, so a refresh costs what the agent wrote since the last one and
//! a multi-megabyte transcript is read once, not on every poll. A replaced or
//! truncated file (new inode, size below the offset) is read again from byte
//! 0; what was already summed is kept and the result says so.
//!
//! Attribution works from facts the container gives us: which session holds
//! the file open (`/proc/<pid>/fd`), when each session process started
//! (`/proc/<pid>/stat`) and when the transcript was first and last written.
//! When those facts leave more than one session, the transcript is reported
//! as belonging to the agent, not to a session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapter::{ExecRequest, RuntimeAdapter};
use crate::files::{
    FileEvent, FileTouch, OPENCODE_DUMP_CAP, aggregate, opencode_files_command, parse_claude,
    parse_codex, parse_opencode, parse_pi,
};
use crate::model::AgentKind;
use crate::sandbox::CONTAINER_HOME;
use crate::usage::{
    ClaudeCall, PriceTable, Sample, Usage, claude_call, claude_usage_from_calls, codex_totals,
    codex_usage_between, opencode_sample, opencode_usage_command, pi_sample, usage_from_samples,
};
use crate::{Error, Result};

/// `$0` of the listing and tail scripts, filtered from the event stream by
/// the shared `sandbox-watcher-` prefix.
pub const TRANSCRIPT_SENTINEL: &str = "sandbox-watcher-transcript";
/// `$0` of the session clock script.
pub const CLOCK_SENTINEL: &str = "sandbox-watcher-clock";
/// `$0` of the chunk reader.
pub const CHUNK_SENTINEL: &str = "sandbox-watcher-chunk";

/// Bytes read per round trip.
pub const CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// Round trips per transcript and sync, so one huge file cannot hold a
/// refresh for long; the rest is read on the next refresh.
const MAX_CHUNKS_PER_SYNC: usize = 4;
/// Transcripts listed per agent, newest first.
const LISTED_PER_AGENT: usize = 12;
/// Clock differences tolerated between process start and transcript times.
pub(crate) const SLACK: Duration = Duration::seconds(5);
/// A transcript first written this soon after one session started, and after
/// no other, belongs to that session.
const START_WINDOW: Duration = Duration::seconds(120);

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// One transcript as the listing saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTranscript {
    pub agent: AgentKind,
    /// Path inside the container; for OpenCode the session's message directory.
    pub path: String,
    pub mtime: Option<DateTime<Utc>>,
    pub size: u64,
    pub inode: u64,
}

pub(crate) fn agent_from_key(key: &str) -> Option<AgentKind> {
    match key {
        "claude" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "opencode" => Some(AgentKind::OpenCode),
        "pi" => Some(AgentKind::Pi),
        _ => None,
    }
}

/// Lists the newest transcripts per agent as `agent|mtime|size|inode|path`.
/// Locations: Claude Code `~/.claude/projects/<cwd>/<session>.jsonl`,
/// Codex `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`,
/// Pi `~/.pi/agent/sessions/<cwd>/<session>.jsonl`,
/// OpenCode `~/.local/share/opencode/storage/message/<session>/` (a directory,
/// size and inode 0).
pub fn list_command() -> Vec<String> {
    let home = CONTAINER_HOME;
    let script = format!(
        r#"list() {{ a=$1; shift; ls -t "$@" 2>/dev/null | head -n {n} | while IFS= read -r f; do [ -n "$f" ] && printf '%s|%s|%s\n' "$a" "$(stat -c '%Y|%s|%i' "$f" 2>/dev/null || echo '0|0|0')" "$f"; done; }}
list claude {home}/.claude/projects/*/*.jsonl
list codex {home}/.codex/sessions/*/*/*/*.jsonl {home}/.codex/sessions/*.jsonl
list pi {home}/.pi/agent/sessions/*/*.jsonl
ls -td {home}/.local/share/opencode/storage/message/*/ 2>/dev/null | head -n {n} | while IFS= read -r d; do d=${{d%/}}; printf 'opencode|%s|0|0|%s\n' "$(stat -c %Y "$d" 2>/dev/null || echo 0)" "$d"; done
exit 0"#,
        n = LISTED_PER_AGENT
    );
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

fn epoch(value: &str) -> Option<DateTime<Utc>> {
    let t = value.trim().parse::<i64>().ok()?;
    (t > 0).then(|| Utc.timestamp_opt(t, 0).single()).flatten()
}

/// Parse [`list_command`] output. Newest first within an agent.
pub fn parse_listing(output: &str) -> Vec<ListedTranscript> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(5, '|');
            let agent = agent_from_key(parts.next()?)?;
            let mtime = epoch(parts.next()?);
            let size = parts.next()?.trim().parse().unwrap_or(0);
            let inode = parts.next()?.trim().parse().unwrap_or(0);
            let path = parts.next()?.trim();
            (!path.is_empty()).then(|| ListedTranscript {
                agent,
                path: path.to_string(),
                mtime,
                size,
                inode,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Session clock: when each session process started, what it holds open
// ---------------------------------------------------------------------------

/// What the container knows about one session process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionClock {
    pub started_at: Option<DateTime<Utc>>,
    /// Transcript paths the process has open.
    pub open_files: Vec<String>,
}

/// Script printing `C|now|hz|uptime`, then `S|pid|starttime` per session pid
/// and `F|pid|path` for every transcript the process holds open. Pids are
/// passed as arguments, never spliced into the script.
pub fn clock_command(pids: &[u32]) -> Vec<String> {
    let script = r#"hz=$(getconf CLK_TCK 2>/dev/null || echo 100); up=$(cut -d' ' -f1 /proc/uptime 2>/dev/null || echo 0); now=$(date +%s); echo "C|$now|$hz|$up"; pids=$*; for p in $pids; do st=$(cat "/proc/$p/stat" 2>/dev/null) || continue; rest=${st##*)}; set -- $rest; echo "S|$p|${20}"; for fd in /proc/$p/fd/*; do l=$(readlink "$fd" 2>/dev/null) || continue; case "$l" in *.jsonl) echo "F|$p|$l";; esac; done; done; exit 0"#;
    let mut command = vec![
        "sh".to_string(),
        "-c".to_string(),
        script.to_string(),
        CLOCK_SENTINEL.to_string(),
    ];
    command.extend(pids.iter().map(u32::to_string));
    command
}

/// Parse [`clock_command`] output. Start times are on the container's clock,
/// like the transcript timestamps they are compared with.
pub fn parse_clock(output: &str) -> HashMap<u32, SessionClock> {
    let mut hz = 100f64;
    let mut uptime = 0f64;
    let mut now: Option<i64> = None;
    let mut out: HashMap<u32, SessionClock> = HashMap::new();
    for line in output.lines() {
        let mut parts = line.splitn(4, '|');
        match parts.next() {
            Some("C") => {
                let mut field = || parts.next().and_then(|v| v.trim().parse::<f64>().ok());
                now = field().map(|v| v as i64);
                hz = field().filter(|v| *v > 0.0).unwrap_or(100.0);
                uptime = field().unwrap_or(0.0);
            }
            Some("S") => {
                let (Some(pid), Some(start)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let Ok(pid) = pid.trim().parse::<u32>() else {
                    continue;
                };
                let start_ticks = start.trim().parse::<f64>().ok();
                let started_at = match (now, start_ticks) {
                    (Some(now), Some(ticks)) if uptime > 0.0 && hz > 0.0 => {
                        let secs = (now as f64 - uptime + ticks / hz).round() as i64;
                        Utc.timestamp_opt(secs, 0).single()
                    }
                    _ => None,
                };
                out.entry(pid).or_default().started_at = started_at;
            }
            Some("F") => {
                let (Some(pid), Some(path)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let Ok(pid) = pid.trim().parse::<u32>() else {
                    continue;
                };
                out.entry(pid)
                    .or_default()
                    .open_files
                    .push(path.trim().to_string());
            }
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Attribution
// ---------------------------------------------------------------------------

/// Why a transcript is tied to one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evidence {
    /// The session process holds the file open.
    OpenFile,
    /// It is the only session of this agent that was running when the
    /// transcript was last written.
    OnlySession,
    /// The transcript was first written right after this session started.
    StartTime,
}

/// Which running session a transcript belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Attribution {
    Session {
        pid: u32,
        evidence: Evidence,
    },
    /// More than one session of the agent could have written it. Usage and
    /// files count for the agent in this container, not for one session.
    Agent {
        candidates: Vec<u32>,
    },
}

impl Attribution {
    /// Sessions the transcript may belong to.
    pub fn pids(&self) -> Vec<u32> {
        match self {
            Attribution::Session { pid, .. } => vec![*pid],
            Attribution::Agent { candidates } => candidates.clone(),
        }
    }
}

/// A running session with what the clock script found out about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStart {
    pub pid: u32,
    pub agent: AgentKind,
    pub started_at: Option<DateTime<Utc>>,
    pub open_files: Vec<String>,
}

/// Decide who wrote a transcript. `last_written` is the newest line time when
/// the whole file was read, else the file's mtime. `None` means no running
/// session of the agent can have written it: it predates them all.
pub fn attribute(
    agent: AgentKind,
    path: &str,
    first_at: Option<DateTime<Utc>>,
    last_written: Option<DateTime<Utc>>,
    sessions: &[SessionStart],
) -> Option<Attribution> {
    let same: Vec<&SessionStart> = sessions.iter().filter(|s| s.agent == agent).collect();
    if same.is_empty() {
        return None;
    }
    if let Some(holder) = same.iter().find(|s| s.open_files.iter().any(|f| f == path)) {
        return Some(Attribution::Session {
            pid: holder.pid,
            evidence: Evidence::OpenFile,
        });
    }
    let candidates: Vec<&SessionStart> = same
        .iter()
        .copied()
        // A session writing another file is not writing this one.
        .filter(|s| s.open_files.is_empty())
        .filter(|s| match (s.started_at, last_written) {
            (Some(start), Some(last)) => last + SLACK >= start,
            _ => true,
        })
        .collect();
    match candidates.as_slice() {
        [] => None,
        [only] => Some(Attribution::Session {
            pid: only.pid,
            evidence: Evidence::OnlySession,
        }),
        many => {
            if let Some(first) = first_at {
                let near: Vec<&&SessionStart> = many
                    .iter()
                    .filter(|s| {
                        s.started_at.is_some_and(|start| {
                            first + SLACK >= start && first <= start + START_WINDOW
                        })
                    })
                    .collect();
                if let [one] = near.as_slice() {
                    return Some(Attribution::Session {
                        pid: one.pid,
                        evidence: Evidence::StartTime,
                    });
                }
            }
            let mut pids: Vec<u32> = many.iter().map(|s| s.pid).collect();
            pids.sort_unstable();
            Some(Attribution::Agent { candidates: pids })
        }
    }
}

// ---------------------------------------------------------------------------
// Incremental state per transcript
// ---------------------------------------------------------------------------

/// How much of a transcript the numbers cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Coverage {
    /// Every complete line of the file has been read.
    pub complete: bool,
    /// Bytes not read yet (including a line still being written).
    pub pending_bytes: u64,
    /// Bytes of lines longer than one chunk that were dropped unparsed.
    pub skipped_bytes: u64,
    /// Times the file was replaced or truncated. Earlier totals were kept.
    pub restarts: u32,
}

#[derive(Debug, Clone)]
enum UsageLog {
    Claude(Vec<(Option<DateTime<Utc>>, ClaudeCall)>),
    Codex {
        model: String,
        totals: Vec<(Option<DateTime<Utc>>, Usage)>,
    },
    Samples(Vec<Sample>),
}

#[derive(Debug, Clone)]
struct TranscriptState {
    agent: AgentKind,
    offset: u64,
    size: u64,
    inode: u64,
    /// Inside a line longer than a chunk: discarding until the next newline.
    skipping: bool,
    skipped_bytes: u64,
    restarts: u32,
    /// The last read reached the end of the file.
    caught_up: bool,
    first_at: Option<DateTime<Utc>>,
    last_at: Option<DateTime<Utc>>,
    log: UsageLog,
    files: Vec<FileEvent>,
}

/// Facts about a transcript after a sync, for attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptFacts {
    pub first_at: Option<DateTime<Utc>>,
    pub last_at: Option<DateTime<Utc>>,
    pub coverage: Coverage,
}

fn at_or_after(at: Option<DateTime<Utc>>, since: Option<DateTime<Utc>>) -> bool {
    match (at, since) {
        (Some(at), Some(since)) => at >= since,
        _ => true,
    }
}

impl TranscriptState {
    fn new(agent: AgentKind, listed: &ListedTranscript) -> Self {
        Self {
            agent,
            offset: 0,
            size: listed.size,
            inode: listed.inode,
            skipping: false,
            skipped_bytes: 0,
            restarts: 0,
            caught_up: false,
            first_at: None,
            last_at: None,
            log: match agent {
                AgentKind::ClaudeCode => UsageLog::Claude(vec![]),
                AgentKind::Codex => UsageLog::Codex {
                    model: "unknown".into(),
                    totals: vec![],
                },
                _ => UsageLog::Samples(vec![]),
            },
            files: vec![],
        }
    }

    /// The file was replaced or cut: read it again from the start, keeping
    /// what was summed so far.
    fn restart(&mut self, inode: u64) {
        self.offset = 0;
        self.inode = inode;
        self.skipping = false;
        self.caught_up = false;
        self.restarts += 1;
    }

    fn note_time(&mut self, at: Option<DateTime<Utc>>) {
        if at.is_none() {
            return;
        }
        if self.first_at.is_none() || at < self.first_at {
            self.first_at = at;
        }
        if self.last_at.is_none() || at > self.last_at {
            self.last_at = at;
        }
    }

    /// Fold one complete transcript line in.
    fn ingest(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let at = crate::activity::timestamp_of(&value);
        match &mut self.log {
            UsageLog::Claude(calls) => {
                if let Some(call) = claude_call(&value) {
                    calls.push((at, call));
                }
                self.files.extend(parse_claude(line));
            }
            UsageLog::Codex { model, totals } => {
                if let Some(m) = value
                    .pointer("/payload/model")
                    .or_else(|| value.get("model"))
                    .and_then(Value::as_str)
                    .filter(|m| !m.is_empty())
                {
                    *model = m.to_string();
                }
                if let Some(t) = codex_totals(&value) {
                    totals.push((at, t));
                }
                self.files.extend(parse_codex(line));
            }
            UsageLog::Samples(samples) => {
                if let Some(s) = pi_sample(&value) {
                    samples.push(s);
                }
                self.files.extend(parse_pi(line));
            }
        }
        self.note_time(at);
    }

    /// Consume a raw chunk read at `offset`. Returns the bytes consumed:
    /// everything up to the last newline, or the whole chunk while skipping
    /// an oversized line.
    fn apply(&mut self, raw: &str) -> u64 {
        let full = raw.len() >= CHUNK_BYTES;
        let mut consumed = 0usize;
        let mut text = raw;
        if self.skipping {
            match text.find('\n') {
                Some(i) => {
                    self.skipped_bytes += (i + 1) as u64;
                    consumed = i + 1;
                    text = &text[consumed..];
                    self.skipping = false;
                }
                None => {
                    let bytes = if full { CHUNK_BYTES } else { raw.len() };
                    self.skipped_bytes += bytes as u64;
                    return bytes as u64;
                }
            }
        }
        match text.rfind('\n') {
            Some(i) => {
                for line in text[..i].lines() {
                    self.ingest(line);
                }
                consumed += i + 1;
            }
            None if full => {
                // One line longer than a chunk: drop it rather than stall.
                let rest = CHUNK_BYTES - consumed;
                self.skipped_bytes += rest as u64;
                self.skipping = true;
                consumed = CHUNK_BYTES;
            }
            None => {}
        }
        consumed as u64
    }

    fn coverage(&self) -> Coverage {
        Coverage {
            complete: self.caught_up,
            pending_bytes: self.size.saturating_sub(self.offset),
            skipped_bytes: self.skipped_bytes,
            restarts: self.restarts,
        }
    }

    fn facts(&self) -> TranscriptFacts {
        TranscriptFacts {
            first_at: self.first_at,
            last_at: self.last_at,
            coverage: self.coverage(),
        }
    }

    /// Tokens and cost of the lines written at or after `since` (all lines
    /// for `None`; lines without a time always count).
    fn usage(&self, since: Option<DateTime<Utc>>, prices: &PriceTable) -> Usage {
        match &self.log {
            UsageLog::Claude(calls) => claude_usage_from_calls(
                calls
                    .iter()
                    .filter(|(at, _)| at_or_after(*at, since))
                    .map(|(_, c)| c.clone())
                    .collect(),
                prices,
            ),
            UsageLog::Codex { model, totals } => {
                let last = totals
                    .last()
                    .map(|(_, t)| t.clone())
                    .unwrap_or_else(|| Usage::new(AgentKind::Codex));
                let base = since.and_then(|since| {
                    totals
                        .iter()
                        .rev()
                        .find(|(at, _)| at.is_some_and(|at| at < since))
                        .map(|(_, t)| t)
                });
                codex_usage_between(base, &last, model, prices)
            }
            UsageLog::Samples(samples) => usage_from_samples(
                self.agent,
                samples.iter().filter(|s| at_or_after(s.at, since)),
            ),
        }
    }

    fn files(&self, since: Option<DateTime<Utc>>, project_dir: Option<&str>) -> Vec<FileTouch> {
        let events: Vec<FileEvent> = self
            .files
            .iter()
            .filter(|e| at_or_after(e.at, since))
            .cloned()
            .collect();
        aggregate(&events, project_dir)
    }
}

// ---------------------------------------------------------------------------
// Commands for one transcript
// ---------------------------------------------------------------------------

/// Reads `size|inode` and then the bytes of `path` from `offset` on, at most
/// one chunk. Exit code 3 when the file is gone. Path and offset are script
/// arguments, never spliced in.
pub fn chunk_command(path: &str, offset: u64) -> Vec<String> {
    let script = format!(
        r#"f=$1; o=$2; s=$(stat -c '%s|%i' "$f" 2>/dev/null) || exit 3; echo "S|$s"; tail -c +$((o+1)) "$f" 2>/dev/null | head -c {CHUNK_BYTES}"#
    );
    vec![
        "sh".into(),
        "-c".into(),
        script,
        CHUNK_SENTINEL.into(),
        path.to_string(),
        offset.to_string(),
    ]
}

/// Split [`chunk_command`] output into `(size, inode, raw)`.
pub fn parse_chunk(output: &str) -> Option<(u64, u64, &str)> {
    let (head, raw) = output.split_once('\n').unwrap_or((output, ""));
    let mut parts = head.strip_prefix("S|")?.split('|');
    let size = parts.next()?.trim().parse().ok()?;
    let inode = parts.next()?.trim().parse().unwrap_or(0);
    Some((size, inode, raw))
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Read position and running totals per transcript, kept between refreshes.
/// Keys are a scope (the container key) and the transcript path.
#[derive(Default)]
pub struct TranscriptCache {
    states: Mutex<HashMap<(String, String), TranscriptState>>,
}

async fn run(
    adapter: &Arc<dyn RuntimeAdapter>,
    id: &str,
    command: Vec<String>,
) -> Result<(i64, String)> {
    let out = adapter.exec(id, ExecRequest::new(command)).await?;
    Ok((out.exit_code, out.stdout))
}

impl TranscriptCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), TranscriptState>> {
        self.states.lock().expect("transcript cache poisoned")
    }

    /// Bring the state of one listed transcript up to date, reading at most
    /// [`MAX_CHUNKS_PER_SYNC`] chunks. Concurrent syncs of the same file are
    /// safe: a read whose starting offset no longer matches is discarded.
    pub async fn sync(
        &self,
        adapter: &Arc<dyn RuntimeAdapter>,
        container_id: &str,
        scope: &str,
        listed: &ListedTranscript,
    ) -> Result<TranscriptFacts> {
        let key = (scope.to_string(), listed.path.clone());
        if listed.agent == AgentKind::OpenCode {
            return self.sync_opencode(adapter, container_id, key, listed).await;
        }
        for _ in 0..MAX_CHUNKS_PER_SYNC {
            let offset = {
                let mut states = self.lock();
                let state = states
                    .entry(key.clone())
                    .or_insert_with(|| TranscriptState::new(listed.agent, listed));
                let replaced = state.inode != 0 && listed.inode != 0 && state.inode != listed.inode;
                if replaced || listed.size < state.offset {
                    state.restart(listed.inode);
                }
                state.size = state.size.max(listed.size);
                if state.offset >= listed.size && state.caught_up {
                    return Ok(state.facts());
                }
                state.offset
            };
            let (code, out) =
                run(adapter, container_id, chunk_command(&listed.path, offset)).await?;
            if code == 3 {
                self.lock().remove(&key);
                return Err(Error::Protocol(format!("{} is gone", listed.path)));
            }
            if code != 0 {
                return Err(Error::Protocol(format!(
                    "reading {} exited with {code}",
                    listed.path
                )));
            }
            let Some((size, inode, raw)) = parse_chunk(&out) else {
                return Err(Error::Protocol(format!(
                    "unexpected chunk header for {}",
                    listed.path
                )));
            };
            let mut states = self.lock();
            let Some(state) = states.get_mut(&key) else {
                return Err(Error::Protocol(format!("{} was dropped", listed.path)));
            };
            if state.offset != offset {
                // Another sync got there first; its result stands.
                return Ok(state.facts());
            }
            if state.inode != 0 && inode != 0 && inode != state.inode {
                state.restart(inode);
                continue;
            }
            state.inode = inode;
            state.size = size.max(offset);
            let consumed = state.apply(raw);
            state.offset += consumed;
            state.caught_up = raw.len() < CHUNK_BYTES;
            if state.caught_up {
                return Ok(state.facts());
            }
        }
        let states = self.lock();
        states
            .get(&key)
            .map(TranscriptState::facts)
            .ok_or_else(|| Error::Protocol(format!("{} was dropped", listed.path)))
    }

    /// OpenCode rewrites message files as a reply completes, so its store is
    /// dumped whole each time; it is small per message.
    async fn sync_opencode(
        &self,
        adapter: &Arc<dyn RuntimeAdapter>,
        container_id: &str,
        key: (String, String),
        listed: &ListedTranscript,
    ) -> Result<TranscriptFacts> {
        let (messages, parts) = futures::join!(
            run(adapter, container_id, opencode_usage_command(&listed.path)),
            run(adapter, container_id, opencode_files_command(&listed.path)),
        );
        let (code, messages) = messages?;
        if code != 0 {
            return Err(Error::Protocol(format!(
                "reading {} exited with {code}",
                listed.path
            )));
        }
        let (_, parts) = parts?;
        let mut state = TranscriptState::new(AgentKind::OpenCode, listed);
        let mut samples = Vec::new();
        for line in messages.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let at = crate::activity::ms_to_time(v.pointer("/time/created"));
            state.note_time(at);
            if let Some(s) = opencode_sample(&v) {
                samples.push(s);
            }
        }
        state.log = UsageLog::Samples(samples);
        state.files = parse_opencode(&parts);
        state.caught_up = parts.len() < OPENCODE_DUMP_CAP;
        state.offset = state.size;
        let facts = state.facts();
        self.lock().insert(key, state);
        Ok(facts)
    }

    /// Facts of a synced transcript, if known.
    pub fn facts(&self, scope: &str, path: &str) -> Option<TranscriptFacts> {
        self.lock()
            .get(&(scope.to_string(), path.to_string()))
            .map(TranscriptState::facts)
    }

    /// Usage of the lines written at or after `since` (`None`: the whole file).
    pub fn usage(
        &self,
        scope: &str,
        path: &str,
        since: Option<DateTime<Utc>>,
        prices: &PriceTable,
    ) -> Option<Usage> {
        self.lock()
            .get(&(scope.to_string(), path.to_string()))
            .map(|s| s.usage(since, prices))
    }

    /// Files touched at or after `since` (`None`: the whole file).
    pub fn files(
        &self,
        scope: &str,
        path: &str,
        since: Option<DateTime<Utc>>,
        project_dir: Option<&str>,
    ) -> Vec<FileTouch> {
        self.lock()
            .get(&(scope.to_string(), path.to_string()))
            .map(|s| s.files(since, project_dir))
            .unwrap_or_default()
    }

    /// Forget transcripts of `scope` that are not in `keep`.
    pub fn retain(&self, scope: &str, keep: &[&str]) {
        self.lock()
            .retain(|(s, p), _| s != scope || keep.contains(&p.as_str()));
    }

    /// Forget every scope not in `scopes` (containers that went away).
    pub fn retain_scopes(&self, scopes: &[&str]) {
        self.lock().retain(|(s, _), _| scopes.contains(&s.as_str()));
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::default_prices;

    fn t(s: &str) -> Option<DateTime<Utc>> {
        Some(DateTime::parse_from_rfc3339(s).unwrap().into())
    }

    fn listed(agent: AgentKind, path: &str, size: u64, inode: u64) -> ListedTranscript {
        ListedTranscript {
            agent,
            path: path.into(),
            mtime: t("2026-10-30T10:00:00Z"),
            size,
            inode,
        }
    }

    #[test]
    fn parses_listing_with_size_and_inode() {
        let out = "claude|1761818400|5120|77|/home/sandbox/.claude/projects/-p/a.jsonl\n\
codex|0|0|0|/home/sandbox/.codex/sessions/2026/10/30/rollout-x.jsonl\n\
opencode|1761818500|0|0|/home/sandbox/.local/share/opencode/storage/message/ses_1\n\
junk\n";
        let list = parse_listing(out);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].agent, AgentKind::ClaudeCode);
        assert_eq!((list[0].size, list[0].inode), (5120, 77));
        assert_eq!(list[0].mtime.unwrap().timestamp(), 1_761_818_400);
        assert!(list[1].mtime.is_none(), "0 means unknown");
        assert_eq!(list[2].agent, AgentKind::OpenCode);
        assert!(list_command().last().unwrap() == TRANSCRIPT_SENTINEL);
        assert!(list_command()[2].contains("stat -c '%Y|%s|%i'"));
    }

    #[test]
    fn clock_gives_start_times_and_open_files() {
        let cmd = clock_command(&[4242, 4300]);
        assert_eq!(cmd[3], CLOCK_SENTINEL);
        assert_eq!(&cmd[4..], &["4242", "4300"]);
        // now = 10:00:00Z, uptime 5000 s, 100 Hz: 4242 started at tick 400000 = 4000 s after boot.
        let out = "C|1761818400|100|5000.25\nS|4242|400000\nF|4242|/home/sandbox/.codex/sessions/2026/10/30/rollout-a.jsonl\nS|4300|420000\nS|bad|1\n";
        let clock = parse_clock(out);
        assert_eq!(clock.len(), 2);
        assert_eq!(
            clock[&4242].started_at.unwrap().timestamp(),
            1_761_818_400 - 5000 + 4000
        );
        assert_eq!(clock[&4242].open_files.len(), 1);
        assert_eq!(
            clock[&4300].started_at.unwrap().timestamp(),
            1_761_818_400 - 5000 + 4200
        );
        assert!(clock[&4300].open_files.is_empty());
        assert!(parse_clock("S|1|2\n")[&1].started_at.is_none(), "no header");
    }

    fn session(pid: u32, started: &str, open: &[&str]) -> SessionStart {
        SessionStart {
            pid,
            agent: AgentKind::ClaudeCode,
            started_at: t(started),
            open_files: open.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn attribution_prefers_open_file_then_only_session_then_start_time() {
        let a = session(1, "2026-10-30T09:00:00Z", &[]);
        let b = session(2, "2026-10-30T09:30:00Z", &[]);
        let path = "/t/x.jsonl";
        // One session: whatever it wrote since it started is its own.
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                path,
                t("2026-10-30T09:05:00Z"),
                t("2026-10-30T09:50:00Z"),
                std::slice::from_ref(&a)
            ),
            Some(Attribution::Session {
                pid: 1,
                evidence: Evidence::OnlySession
            })
        );
        // Written before every session started: stale.
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                path,
                t("2026-10-30T08:00:00Z"),
                t("2026-10-30T08:30:00Z"),
                &[a.clone(), b.clone()]
            ),
            None
        );
        // Two sessions, transcript begun right after the second started.
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                path,
                t("2026-10-30T09:30:40Z"),
                t("2026-10-30T09:50:00Z"),
                &[a.clone(), b.clone()]
            ),
            Some(Attribution::Session {
                pid: 2,
                evidence: Evidence::StartTime
            })
        );
        // Two sessions, transcript begun long after both: ambiguous.
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                path,
                t("2026-10-30T09:45:00Z"),
                t("2026-10-30T09:50:00Z"),
                &[b.clone(), a.clone()]
            ),
            Some(Attribution::Agent {
                candidates: vec![1, 2]
            })
        );
        // Only written while the first ran alone: the first.
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                path,
                t("2026-10-30T09:05:00Z"),
                t("2026-10-30T09:20:00Z"),
                &[a.clone(), b.clone()]
            ),
            Some(Attribution::Session {
                pid: 1,
                evidence: Evidence::OnlySession
            })
        );
        // An open file descriptor settles it, whatever the times say.
        let holder = session(2, "2026-10-30T09:30:00Z", &[path]);
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                path,
                t("2026-10-30T09:05:00Z"),
                t("2026-10-30T09:20:00Z"),
                &[a.clone(), holder.clone()]
            ),
            Some(Attribution::Session {
                pid: 2,
                evidence: Evidence::OpenFile
            })
        );
        // A session holding another file open is not a candidate.
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                "/t/other.jsonl",
                t("2026-10-30T09:45:00Z"),
                t("2026-10-30T09:50:00Z"),
                &[a.clone(), holder]
            ),
            Some(Attribution::Session {
                pid: 1,
                evidence: Evidence::OnlySession
            })
        );
        // Unknown start times keep every session in the running.
        let unknown = SessionStart {
            started_at: None,
            ..a.clone()
        };
        assert_eq!(
            attribute(
                AgentKind::ClaudeCode,
                path,
                None,
                t("2026-10-30T09:50:00Z"),
                &[unknown, b]
            ),
            Some(Attribution::Agent {
                candidates: vec![1, 2]
            })
        );
        // Another agent's sessions do not count.
        assert_eq!(attribute(AgentKind::Codex, path, None, None, &[a]), None);
    }

    const LINE_A: &str = r#"{"type":"assistant","message":{"model":"claude-sonnet-4-5","id":"msg_a","usage":{"input_tokens":100,"output_tokens":10},"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/a.ts"}}]},"timestamp":"2026-10-30T09:00:00Z"}"#;
    const LINE_B: &str = r#"{"type":"assistant","message":{"model":"claude-sonnet-4-5","id":"msg_b","usage":{"input_tokens":200,"output_tokens":20},"content":[{"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/p/a.ts","old_string":"x","new_string":"y"}}]},"timestamp":"2026-10-30T10:00:00Z"}"#;

    #[test]
    fn state_sums_lines_and_filters_by_time() {
        let l = listed(AgentKind::ClaudeCode, "/t/x.jsonl", 0, 1);
        let mut s = TranscriptState::new(AgentKind::ClaudeCode, &l);
        let text = format!("{LINE_A}\n{LINE_B}\n{{\"type\":\"assist");
        let consumed = s.apply(&text);
        assert_eq!(consumed as usize, LINE_A.len() + LINE_B.len() + 2);
        assert_eq!(s.first_at, t("2026-10-30T09:00:00Z"));
        assert_eq!(s.last_at, t("2026-10-30T10:00:00Z"));
        let all = s.usage(None, &default_prices());
        assert_eq!(
            (all.input_tokens, all.output_tokens, all.turns),
            (300, 30, 2)
        );
        let later = s.usage(t("2026-10-30T09:30:00Z"), &default_prices());
        assert_eq!((later.input_tokens, later.turns), (200, 1));
        assert_eq!(s.files(None, None)[0].reads, 1);
        assert_eq!(s.files(t("2026-10-30T09:30:00Z"), None)[0].reads, 0);
        // The cut line is read again with the rest.
        let rest = format!("ant\",\"message\":{{}}}}\n{LINE_A}\n");
        let text2 = format!("{{\"type\":\"assist{rest}");
        s.apply(&text2);
        assert_eq!(
            s.usage(None, &default_prices()).turns,
            2,
            "msg_a repeated once per id"
        );
    }

    #[test]
    fn oversized_lines_are_skipped_not_stalled() {
        let l = listed(AgentKind::Pi, "/t/p.jsonl", 0, 1);
        let mut s = TranscriptState::new(AgentKind::Pi, &l);
        let huge = "x".repeat(CHUNK_BYTES);
        assert_eq!(s.apply(&huge) as usize, CHUNK_BYTES);
        assert!(s.skipping);
        let tail = "yyy\n{\"type\":\"message\",\"timestamp\":\"2026-10-30T10:00:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":5,\"output\":1,\"cost\":{\"total\":0.1}}}}\n";
        assert_eq!(s.apply(tail) as usize, tail.len());
        assert!(!s.skipping);
        assert_eq!(s.skipped_bytes as usize, CHUNK_BYTES + 4);
        assert_eq!(s.usage(None, &default_prices()).input_tokens, 5);
    }

    #[test]
    fn codex_usage_is_a_difference_of_totals_since_the_session() {
        let l = listed(AgentKind::Codex, "/t/r.jsonl", 0, 1);
        let mut s = TranscriptState::new(AgentKind::Codex, &l);
        let lines = [
            r#"{"timestamp":"2026-10-30T09:00:00Z","type":"turn_context","payload":{"cwd":"/p","model":"gpt-5-codex"}}"#,
            r#"{"timestamp":"2026-10-30T09:10:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":100,"reasoning_output_tokens":0}}}}"#,
            r#"{"timestamp":"2026-10-30T10:10:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3000,"cached_input_tokens":500,"output_tokens":400,"reasoning_output_tokens":50}}}}"#,
        ];
        s.apply(&format!("{}\n", lines.join("\n")));
        let whole = s.usage(None, &default_prices());
        assert_eq!((whole.input_tokens, whole.output_tokens), (2500, 400));
        assert_eq!(whole.models, vec!["gpt-5-codex"]);
        let resumed = s.usage(t("2026-10-30T10:00:00Z"), &default_prices());
        assert_eq!(
            (
                resumed.input_tokens,
                resumed.cache_read_tokens,
                resumed.output_tokens
            ),
            (1500, 500, 300)
        );
    }

    #[test]
    fn chunk_command_passes_path_and_offset_as_arguments() {
        let c = chunk_command("/t/it's.jsonl", 4096);
        assert_eq!(c[3], CHUNK_SENTINEL);
        assert_eq!(c[4], "/t/it's.jsonl");
        assert_eq!(c[5], "4096");
        assert!(c[2].contains("tail -c +$((o+1))"));
        assert_eq!(parse_chunk("S|120|9\nabc\n"), Some((120, 9, "abc\n")));
        assert_eq!(parse_chunk("S|0|0\n"), Some((0, 0, "")));
        assert!(parse_chunk("nope").is_none());
    }

    #[tokio::test]
    async fn cache_reads_incrementally_from_the_mock() {
        use crate::mock::MockAdapter;
        let mock = MockAdapter::demo();
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(mock.clone());
        let cache = TranscriptCache::default();
        let listing = run(&adapter, "sandbox-web-shop", list_command())
            .await
            .unwrap()
            .1;
        let listed = parse_listing(&listing).remove(0);
        let facts = cache
            .sync(&adapter, "sandbox-web-shop", "k", &listed)
            .await
            .unwrap();
        assert!(facts.coverage.complete, "{facts:?}");
        assert_eq!(facts.coverage.pending_bytes, 0);
        let whole = cache
            .usage("k", &listed.path, None, &default_prices())
            .unwrap();
        assert_eq!(whole.turns, 2);
        assert_eq!(whole.input_tokens, 1500);
        assert_eq!(cache.files("k", &listed.path, None, None).len(), 1);

        // The agent appends a line: only the new bytes are read and summed.
        mock.append_transcript(LINE_B);
        let listing = run(&adapter, "sandbox-web-shop", list_command())
            .await
            .unwrap()
            .1;
        let grown = parse_listing(&listing).remove(0);
        assert!(grown.size > listed.size);
        cache
            .sync(&adapter, "sandbox-web-shop", "k", &grown)
            .await
            .unwrap();
        let whole = cache
            .usage("k", &listed.path, None, &default_prices())
            .unwrap();
        assert_eq!(whole.turns, 3);
        assert_eq!(whole.input_tokens, 1700);
        assert_eq!(mock.chunk_reads(), 2, "one read per sync");

        // Nothing new: no read at all.
        cache
            .sync(&adapter, "sandbox-web-shop", "k", &grown)
            .await
            .unwrap();
        assert_eq!(mock.chunk_reads(), 2);

        // The file is replaced by a shorter one: read again, earlier totals kept.
        mock.replace_transcript(LINE_A);
        let listing = run(&adapter, "sandbox-web-shop", list_command())
            .await
            .unwrap()
            .1;
        let replaced = parse_listing(&listing).remove(0);
        let facts = cache
            .sync(&adapter, "sandbox-web-shop", "k", &replaced)
            .await
            .unwrap();
        assert_eq!(facts.coverage.restarts, 1);
        assert!(facts.coverage.complete);
        let whole = cache
            .usage("k", &listed.path, None, &default_prices())
            .unwrap();
        assert_eq!(
            whole.turns, 4,
            "3 earlier calls plus msg_a from the new file"
        );

        cache.retain("k", &[]);
        assert!(cache.is_empty());
    }
}
