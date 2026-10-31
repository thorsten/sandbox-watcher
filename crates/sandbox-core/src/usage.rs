//! Token usage and cost per agent session, read from the agents' own
//! transcripts inside the container.
//!
//! Pi and OpenCode report cost themselves. Claude Code and Codex report
//! tokens and a model; cost is estimated from a price table the user can
//! edit. Unknown models show tokens only.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::activity::TRANSCRIPT_SENTINEL;
use crate::model::AgentKind;

/// USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

/// Prefix-matched price table: the longest key that prefixes the model id wins.
pub type PriceTable = BTreeMap<String, ModelPrice>;

/// Defaults for models we know prices for. Edit in settings for others.
/// Anthropic list prices as of October 2026; cache writes are the 5-minute
/// rate. Claude Haiku 5.5 uses its rate for prompts up to 100K tokens.
pub fn default_prices() -> PriceTable {
    let p = |input, output, cache_read, cache_write| ModelPrice {
        input,
        output,
        cache_read,
        cache_write,
    };
    BTreeMap::from([
        ("claude-fable-5-1".into(), p(10.0, 50.0, 0.25, 12.5)),
        ("claude-fable-5".into(), p(10.0, 50.0, 1.0, 12.5)),
        ("claude-opus-5-5".into(), p(4.0, 20.0, 0.2, 5.0)),
        ("claude-opus-5".into(), p(5.0, 25.0, 0.5, 6.25)),
        ("claude-opus-4-8".into(), p(5.0, 25.0, 0.5, 6.25)),
        ("claude-opus-4-7".into(), p(5.0, 25.0, 0.5, 6.25)),
        ("claude-opus-4-6".into(), p(5.0, 25.0, 0.5, 6.25)),
        ("claude-opus-4-5".into(), p(5.0, 25.0, 0.5, 6.25)),
        // Opus 4 and 4.1.
        ("claude-opus-4".into(), p(15.0, 75.0, 1.5, 18.75)),
        ("claude-sonnet-5".into(), p(2.0, 10.0, 0.2, 2.5)),
        ("claude-sonnet-4".into(), p(3.0, 15.0, 0.3, 3.75)),
        ("claude-haiku-5-5".into(), p(0.1, 0.5, 0.01, 0.125)),
        ("claude-haiku-4".into(), p(1.0, 5.0, 0.1, 1.25)),
        ("claude-3-5-haiku".into(), p(0.8, 4.0, 0.08, 1.0)),
        ("gpt-5-mini".into(), p(0.25, 2.0, 0.025, 0.0)),
        ("gpt-5-nano".into(), p(0.05, 0.4, 0.005, 0.0)),
        ("gpt-5".into(), p(1.25, 10.0, 0.125, 0.0)),
        ("gpt-4.1-mini".into(), p(0.4, 1.6, 0.1, 0.0)),
        ("gpt-4.1".into(), p(2.0, 8.0, 0.5, 0.0)),
        ("o3".into(), p(2.0, 8.0, 0.5, 0.0)),
        ("o4-mini".into(), p(1.1, 4.4, 0.275, 0.0)),
    ])
}

pub fn price_for(table: &PriceTable, model: &str) -> Option<ModelPrice> {
    let model = model.to_ascii_lowercase();
    table
        .iter()
        .filter(|(prefix, _)| model.starts_with(prefix.as_str()))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, price)| *price)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CostSource {
    /// The agent reported the cost itself.
    Reported,
    /// Computed from tokens and the price table.
    Estimated,
    /// No price for this model.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub agent: AgentKind,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    /// Assistant turns seen (Claude, Pi, OpenCode) or token_count events (Codex).
    pub turns: u64,
    pub cost_usd: Option<f64>,
    pub cost_source: CostSource,
    /// Models seen, most used first.
    pub models: Vec<String>,
}

impl Usage {
    pub(crate) fn new(agent: AgentKind) -> Self {
        Self {
            agent,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            turns: 0,
            cost_usd: None,
            cost_source: CostSource::Unknown,
            models: vec![],
        }
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }

    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.turns += other.turns;
        match (self.cost_usd, other.cost_usd) {
            (Some(a), Some(b)) => self.cost_usd = Some(a + b),
            (None, Some(b)) if self.turns == other.turns => self.cost_usd = Some(b),
            _ => {}
        }
        if other.cost_source == CostSource::Unknown && other.total_tokens() > 0 {
            self.cost_source = CostSource::Unknown;
        } else if self.cost_source != CostSource::Unknown
            && other.cost_source == CostSource::Estimated
        {
            self.cost_source = CostSource::Estimated;
        }
        for m in &other.models {
            if !self.models.contains(m) {
                self.models.push(m.clone());
            }
        }
    }
}

fn u64_of(v: Option<&Value>) -> u64 {
    v.and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64)))
        .unwrap_or(0)
}

fn f64_of(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

fn estimate(usage: &mut Usage, per_model: &BTreeMap<String, Usage>, table: &PriceTable) {
    let mut total = 0.0;
    let mut priced_any = false;
    let mut unpriced_tokens = 0;
    for (model, u) in per_model {
        match price_for(table, model) {
            Some(p) => {
                priced_any = true;
                total += u.input_tokens as f64 * p.input / 1e6
                    + u.output_tokens as f64 * p.output / 1e6
                    + u.cache_read_tokens as f64 * p.cache_read / 1e6
                    + u.cache_write_tokens as f64 * p.cache_write / 1e6;
            }
            None => unpriced_tokens += u.total_tokens(),
        }
    }
    if priced_any && unpriced_tokens == 0 {
        usage.cost_usd = Some(total);
        usage.cost_source = CostSource::Estimated;
    } else if priced_any {
        // Partial price coverage: still show what we can, flagged as unknown.
        usage.cost_usd = Some(total);
        usage.cost_source = CostSource::Unknown;
    } else {
        usage.cost_usd = None;
        usage.cost_source = CostSource::Unknown;
    }
}

fn sort_models(per_model: &BTreeMap<String, Usage>) -> Vec<String> {
    let mut models: Vec<(&String, u64)> = per_model
        .iter()
        .map(|(m, u)| (m, u.total_tokens()))
        .collect();
    models.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    models.into_iter().map(|(m, _)| m.clone()).collect()
}

// ---------------------------------------------------------------------------
// Claude Code: every assistant line carries message.usage and message.model.
// One API message is written as several lines (thinking, text, each
// tool_use) that repeat the same message id and usage, so calls are counted
// once per id.
// ---------------------------------------------------------------------------

/// Tokens of one Claude API call. Lines without a message id (old
/// transcripts) count as calls of their own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClaudeCall {
    pub id: Option<String>,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// Sum calls per model, a repeated id replacing the earlier line.
pub(crate) fn claude_usage_from_calls(calls: Vec<ClaudeCall>, table: &PriceTable) -> Usage {
    let mut unique: Vec<ClaudeCall> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    // Claude Code writes placeholder messages (errors, interrupts) as `<synthetic>`.
    for call in calls.into_iter().filter(|c| c.model != "<synthetic>") {
        match call.id.as_ref().and_then(|id| by_id.get(id)) {
            Some(&i) => unique[i] = call,
            None => {
                if let Some(id) = &call.id {
                    by_id.insert(id.clone(), unique.len());
                }
                unique.push(call);
            }
        }
    }
    let mut usage = Usage::new(AgentKind::ClaudeCode);
    let mut per_model: BTreeMap<String, Usage> = BTreeMap::new();
    for call in &unique {
        let entry = per_model
            .entry(call.model.clone())
            .or_insert_with(|| Usage::new(AgentKind::ClaudeCode));
        entry.input_tokens += call.input;
        entry.output_tokens += call.output;
        entry.cache_read_tokens += call.cache_read;
        entry.cache_write_tokens += call.cache_write;
        entry.turns += 1;
    }
    for u in per_model.values() {
        usage.input_tokens += u.input_tokens;
        usage.output_tokens += u.output_tokens;
        usage.cache_read_tokens += u.cache_read_tokens;
        usage.cache_write_tokens += u.cache_write_tokens;
        usage.turns += u.turns;
    }
    usage.models = sort_models(&per_model);
    estimate(&mut usage, &per_model, table);
    usage
}

/// Extract only the model, message id and usage of each assistant line; the
/// file can be megabytes, the fragments are a few hundred bytes per line.
/// `usage` holds nested objects, so its fragment runs to the end of the line
/// (cut short) and the counters are read from its top level.
pub fn claude_usage_command(path: &str) -> Vec<String> {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    let script = format!(
        r#"grep -F '"type":"assistant"' {quoted} 2>/dev/null | grep -noE '"model":"[^"]*"|"id":"msg_[^"]*"|"usage":\{{.*' | cut -c1-600"#
    );
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

/// First `"key":<number>` in a fragment. Top-level usage counters come before
/// any nested object, so the first match is the right one.
fn number_after(fragment: &str, key: &str) -> u64 {
    let needle = format!("\"{key}\":");
    fragment
        .find(&needle)
        .map(|i| &fragment[i + needle.len()..])
        .map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// Parse the grep output: `<line>:"model":"x"`, `<line>:"id":"msg_…"` and
/// `<line>:"usage":{…` fragments, the first of each kind per line counting.
pub fn parse_claude_usage(output: &str, table: &PriceTable) -> Usage {
    let mut calls = Vec::new();
    let mut line_no = "";
    let mut model = String::from("unknown");
    let mut model_on_line = false;
    let mut id: Option<String> = None;
    for line in output.lines() {
        let Some((no, fragment)) = line.split_once(':') else {
            continue;
        };
        if no != line_no {
            line_no = no;
            model_on_line = false;
            id = None;
        }
        if let Some(rest) = fragment.strip_prefix("\"model\":\"") {
            if !model_on_line {
                model = rest.trim_end_matches('"').to_string();
                model_on_line = true;
            }
        } else if let Some(rest) = fragment.strip_prefix("\"id\":\"") {
            if id.is_none() {
                id = Some(rest.trim_end_matches('"').to_string());
            }
        } else if fragment.starts_with("\"usage\":{") {
            calls.push(ClaudeCall {
                id: id.clone(),
                model: model.clone(),
                input: number_after(fragment, "input_tokens"),
                output: number_after(fragment, "output_tokens"),
                cache_read: number_after(fragment, "cache_read_input_tokens"),
                cache_write: number_after(fragment, "cache_creation_input_tokens"),
            });
        }
    }
    claude_usage_from_calls(calls, table)
}

/// The call recorded on one full transcript line, if it is an assistant line
/// with usage.
pub(crate) fn claude_call(line: &Value) -> Option<ClaudeCall> {
    if line.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let message = line.get("message")?;
    let u = message.get("usage")?;
    Some(ClaudeCall {
        id: message.get("id").and_then(Value::as_str).map(String::from),
        model: message
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        input: u64_of(u.get("input_tokens")),
        output: u64_of(u.get("output_tokens")),
        cache_read: u64_of(u.get("cache_read_input_tokens")),
        cache_write: u64_of(u.get("cache_creation_input_tokens")),
    })
}

// ---------------------------------------------------------------------------
// Codex: token_count events carry cumulative totals in newer versions.
// ---------------------------------------------------------------------------

pub fn codex_usage_command(path: &str) -> Vec<String> {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    // token_count events without figures (rate limits only) are skipped.
    let script = format!(
        r#"grep -o '"model":"[^"]*"' {quoted} 2>/dev/null | head -1; grep '"token_count"' {quoted} 2>/dev/null | grep '"input_tokens"' | tail -1"#
    );
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

/// Cumulative totals of one Codex `token_count` line, input still including
/// cached tokens. `None` for other lines and for counts without figures.
pub(crate) fn codex_totals(line: &Value) -> Option<Usage> {
    let payload = line.get("payload").unwrap_or(line);
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    // Newer: payload.info.total_token_usage; older: payload fields directly.
    let totals = payload
        .pointer("/info/total_token_usage")
        .or_else(|| payload.get("total_token_usage"))
        .unwrap_or(payload);
    totals.get("input_tokens")?;
    let mut usage = Usage::new(AgentKind::Codex);
    usage.input_tokens = u64_of(totals.get("input_tokens"));
    usage.output_tokens = u64_of(totals.get("output_tokens"));
    usage.cache_read_tokens = u64_of(totals.get("cached_input_tokens"));
    usage.reasoning_tokens = u64_of(totals.get("reasoning_output_tokens"));
    usage.turns = 1;
    Some(usage)
}

/// Usage between two cumulative Codex totals (`since` = `None` for the start
/// of the session), priced for `model`.
pub(crate) fn codex_usage_between(
    since: Option<&Usage>,
    last: &Usage,
    model: &str,
    table: &PriceTable,
) -> Usage {
    let mut usage = last.clone();
    if let Some(since) = since {
        usage.input_tokens = last.input_tokens.saturating_sub(since.input_tokens);
        usage.output_tokens = last.output_tokens.saturating_sub(since.output_tokens);
        usage.cache_read_tokens = last
            .cache_read_tokens
            .saturating_sub(since.cache_read_tokens);
        usage.reasoning_tokens = last.reasoning_tokens.saturating_sub(since.reasoning_tokens);
    }
    // Codex reports cached tokens as part of input; price them separately.
    usage.input_tokens = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
    let mut per_model = BTreeMap::new();
    per_model.insert(model.to_string(), usage.clone());
    usage.models = vec![model.to_string()];
    estimate(&mut usage, &per_model, table);
    usage
}

pub fn parse_codex_usage(output: &str, table: &PriceTable) -> Usage {
    let mut model = String::from("unknown");
    let mut last = None;
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("\"model\":\"") {
            model = rest.trim_end_matches('"').to_string();
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line)
            && let Some(totals) = codex_totals(&v)
        {
            last = Some(totals);
        }
    }
    let last = last.unwrap_or_else(|| Usage::new(AgentKind::Codex));
    codex_usage_between(None, &last, &model, table)
}

// ---------------------------------------------------------------------------
// Pi: assistant messages carry usage with a cost object.
// ---------------------------------------------------------------------------

pub fn pi_usage_command(path: &str) -> Vec<String> {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    // Whole file, capped: Pi sessions are small and usage sits at the end of a line.
    let script = format!("head -c 8388608 {quoted} 2>/dev/null");
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

pub fn parse_pi_usage(text: &str) -> Usage {
    let mut usage = Usage::new(AgentKind::Pi);
    let mut per_model: BTreeMap<String, Usage> = BTreeMap::new();
    let mut cost = 0.0;
    let mut reported = false;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(message) = v.get("message") else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(u) = message.get("usage") else {
            continue;
        };
        let model = message
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let entry = per_model
            .entry(model)
            .or_insert_with(|| Usage::new(AgentKind::Pi));
        entry.input_tokens += u64_of(u.get("input"));
        entry.output_tokens += u64_of(u.get("output"));
        entry.cache_read_tokens += u64_of(u.get("cacheRead"));
        entry.cache_write_tokens += u64_of(u.get("cacheWrite"));
        entry.turns += 1;
        if let Some(total) = f64_of(u.pointer("/cost/total")) {
            cost += total;
            reported = true;
        }
    }
    for u in per_model.values() {
        usage.input_tokens += u.input_tokens;
        usage.output_tokens += u.output_tokens;
        usage.cache_read_tokens += u.cache_read_tokens;
        usage.cache_write_tokens += u.cache_write_tokens;
        usage.turns += u.turns;
    }
    usage.models = sort_models(&per_model);
    if reported {
        usage.cost_usd = Some(cost);
        usage.cost_source = CostSource::Reported;
    }
    usage
}

// ---------------------------------------------------------------------------
// OpenCode: assistant message files carry tokens and cost.
// ---------------------------------------------------------------------------

pub fn opencode_usage_command(session_dir: &str) -> Vec<String> {
    let quoted = format!("'{}'", session_dir.replace('\'', "'\\''"));
    let script = format!(
        r#"for m in {quoted}/*.json; do [ -f "$m" ] && printf '%s\n' "$(tr -d '\n' < "$m")"; done 2>/dev/null"#
    );
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

pub fn parse_opencode_usage(text: &str) -> Usage {
    let mut usage = Usage::new(AgentKind::OpenCode);
    let mut per_model: BTreeMap<String, Usage> = BTreeMap::new();
    let mut cost = 0.0;
    let mut reported = false;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(t) = v.get("tokens") else { continue };
        let model = v
            .get("modelID")
            .or_else(|| v.pointer("/model/modelID"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let entry = per_model
            .entry(model)
            .or_insert_with(|| Usage::new(AgentKind::OpenCode));
        entry.input_tokens += u64_of(t.get("input"));
        entry.output_tokens += u64_of(t.get("output"));
        entry.reasoning_tokens += u64_of(t.get("reasoning"));
        entry.cache_read_tokens += u64_of(t.pointer("/cache/read"));
        entry.cache_write_tokens += u64_of(t.pointer("/cache/write"));
        entry.turns += 1;
        if let Some(c) = f64_of(v.get("cost")) {
            cost += c;
            reported = true;
        }
    }
    for u in per_model.values() {
        usage.input_tokens += u.input_tokens;
        usage.output_tokens += u.output_tokens;
        usage.cache_read_tokens += u.cache_read_tokens;
        usage.cache_write_tokens += u.cache_write_tokens;
        usage.reasoning_tokens += u.reasoning_tokens;
        usage.turns += u.turns;
    }
    usage.models = sort_models(&per_model);
    if reported {
        usage.cost_usd = Some(cost);
        usage.cost_source = CostSource::Reported;
    }
    usage
}

/// Command that extracts usage for an agent's transcript source.
pub fn usage_command(agent: AgentKind, source: &str) -> Option<Vec<String>> {
    match agent {
        AgentKind::ClaudeCode => Some(claude_usage_command(source)),
        AgentKind::Codex => Some(codex_usage_command(source)),
        AgentKind::Pi => Some(pi_usage_command(source)),
        AgentKind::OpenCode => Some(opencode_usage_command(source)),
        AgentKind::Shell | AgentKind::Other => None,
    }
}

pub fn parse_usage(agent: AgentKind, output: &str, table: &PriceTable) -> Option<Usage> {
    match agent {
        AgentKind::ClaudeCode => Some(parse_claude_usage(output, table)),
        AgentKind::Codex => Some(parse_codex_usage(output, table)),
        AgentKind::Pi => Some(parse_pi_usage(output)),
        AgentKind::OpenCode => Some(parse_opencode_usage(output)),
        AgentKind::Shell | AgentKind::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_lookup_prefers_longest_prefix() {
        let t = default_prices();
        assert_eq!(price_for(&t, "gpt-5-mini-2025-08-07").unwrap().input, 0.25);
        assert_eq!(price_for(&t, "gpt-5-codex").unwrap().input, 1.25);
        assert_eq!(
            price_for(&t, "claude-sonnet-4-5-20250929").unwrap().output,
            15.0
        );
        assert!(price_for(&t, "mystery-model").is_none());
    }

    #[test]
    fn claude_versions_resolve_to_their_own_prices() {
        let t = default_prices();
        let input = |model| price_for(&t, model).unwrap().input;
        assert_eq!(input("claude-opus-5-5"), 4.0);
        assert_eq!(input("claude-opus-5"), 5.0);
        assert_eq!(input("claude-opus-4-5-20251101"), 5.0);
        assert_eq!(input("claude-opus-4-1-20250805"), 15.0);
        assert_eq!(input("claude-sonnet-5-5"), 2.0);
        assert_eq!(input("claude-haiku-5-5"), 0.1);
        assert_eq!(input("claude-haiku-4-5-20251001"), 1.0);
        assert_eq!(price_for(&t, "claude-fable-5-1").unwrap().cache_read, 0.25);
        assert_eq!(price_for(&t, "claude-fable-5").unwrap().cache_read, 1.0);
    }

    #[test]
    fn claude_usage_sums_per_model_and_estimates_cost() {
        let out = "1:\"model\":\"claude-sonnet-4-5-20250929\"\n1:\"id\":\"msg_a\"\n1:\"usage\":{\"input_tokens\":1000,\"cache_creation_input_tokens\":2000,\"cache_read_input_tokens\":10000,\"output_tokens\":500}\n2:\"model\":\"claude-sonnet-4-5-20250929\"\n2:\"id\":\"msg_b\"\n2:\"usage\":{\"input_tokens\":100,\"output_tokens\":50}\n";
        let u = parse_claude_usage(out, &default_prices());
        assert_eq!(
            (
                u.input_tokens,
                u.output_tokens,
                u.cache_read_tokens,
                u.cache_write_tokens
            ),
            (1100, 550, 10000, 2000)
        );
        assert_eq!(u.turns, 2);
        assert_eq!(u.models, vec!["claude-sonnet-4-5-20250929"]);
        assert_eq!(u.cost_source, CostSource::Estimated);
        let expected =
            1100.0 * 3.0 / 1e6 + 550.0 * 15.0 / 1e6 + 10000.0 * 0.3 / 1e6 + 2000.0 * 3.75 / 1e6;
        assert!((u.cost_usd.unwrap() - expected).abs() < 1e-9);

        let unknown = parse_claude_usage(
            "1:\"model\":\"claude-future-9\"\n1:\"usage\":{\"input_tokens\":10,\"output_tokens\":1}\n",
            &default_prices(),
        );
        assert_eq!(unknown.cost_source, CostSource::Unknown);
        assert!(unknown.cost_usd.is_none());
    }

    /// Current Claude Code: nested objects inside `usage`, one API message
    /// split over several lines with the same id, and a tool input that
    /// carries its own `model` key.
    #[test]
    fn claude_usage_reads_nested_usage_once_per_message() {
        let lines = [
            r#"{"type":"assistant","message":{"model":"claude-opus-4-1","id":"msg_1","content":[{"type":"thinking","thinking":""}],"usage":{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":5000,"output_tokens":40,"output_tokens_details":{"thinking_tokens":30},"cache_creation":{"ephemeral_1h_input_tokens":100}}},"timestamp":"2026-10-30T10:00:00Z"}"#,
            r#"{"type":"assistant","message":{"model":"claude-opus-4-1","id":"msg_1","content":[{"type":"tool_use","id":"toolu_1","name":"Agent","input":{"model":"haiku","prompt":"x"}}],"usage":{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":5000,"output_tokens":40,"output_tokens_details":{"thinking_tokens":30},"cache_creation":{"ephemeral_1h_input_tokens":100}}},"timestamp":"2026-10-30T10:00:01Z"}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]},"toolUseResult":{"usage":{"input_tokens":999}}}"#,
            r#"{"type":"assistant","message":{"model":"<synthetic>","id":"msg_x","content":[{"type":"text","text":"API Error"}],"usage":{"input_tokens":0,"output_tokens":0}}}"#,
            r#"{"type":"assistant","message":{"model":"claude-opus-4-1","id":"msg_2","content":[{"type":"text","text":"done"}],"usage":{"input_tokens":3,"cache_read_input_tokens":5100,"output_tokens":7,"iterations":[{"input_tokens":1,"output_tokens":1}]}}}"#,
        ];
        // What `claude_usage_command` prints for these lines.
        let grep = sim_claude_grep(&lines);
        let u = parse_claude_usage(&grep, &default_prices());
        assert_eq!(
            (
                u.input_tokens,
                u.output_tokens,
                u.cache_read_tokens,
                u.cache_write_tokens,
                u.turns
            ),
            (5, 47, 10100, 100, 2)
        );
        assert_eq!(u.models, vec!["claude-opus-4-1"]);

        let calls: Vec<ClaudeCall> = lines
            .iter()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| claude_call(&v))
            .collect();
        assert_eq!(claude_usage_from_calls(calls, &default_prices()), u);
    }

    /// Mimic `grep -F '"type":"assistant"' | grep -noE '…' | cut -c1-600`.
    fn sim_claude_grep(lines: &[&str]) -> String {
        let mut out = String::new();
        let assistant = lines.iter().filter(|l| l.contains(r#""type":"assistant""#));
        for (i, line) in assistant.enumerate() {
            let mut rest = *line;
            while !rest.is_empty() {
                let next = ["\"model\":\"", "\"id\":\"msg_", "\"usage\":{"]
                    .iter()
                    .filter_map(|p| rest.find(p).map(|at| (at, *p)))
                    .min();
                let Some((at, prefix)) = next else { break };
                let tail = &rest[at..];
                let fragment = if prefix == "\"usage\":{" {
                    tail
                } else {
                    let close = tail[prefix.len()..].find('"').unwrap() + prefix.len() + 1;
                    &tail[..close]
                };
                let numbered = format!("{}:{fragment}", i + 1);
                out.push_str(&numbered.chars().take(600).collect::<String>());
                out.push('\n');
                rest = &tail[fragment.len()..];
            }
        }
        out
    }

    #[test]
    fn codex_usage_takes_the_last_cumulative_count() {
        let out = "\"model\":\"gpt-5-codex\"\n{\"timestamp\":\"t\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":5000,\"cached_input_tokens\":4000,\"output_tokens\":800,\"reasoning_output_tokens\":300,\"total_tokens\":5800}}}}\n";
        let u = parse_codex_usage(out, &default_prices());
        assert_eq!(
            (
                u.input_tokens,
                u.cache_read_tokens,
                u.output_tokens,
                u.reasoning_tokens
            ),
            (1000, 4000, 800, 300)
        );
        assert_eq!(u.cost_source, CostSource::Estimated);
        let expected = 1000.0 * 1.25 / 1e6 + 800.0 * 10.0 / 1e6 + 4000.0 * 0.125 / 1e6;
        assert!((u.cost_usd.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn pi_and_opencode_report_cost_directly() {
        let pi = "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-sonnet-4-5\",\"content\":[],\"usage\":{\"input\":100,\"output\":20,\"cacheRead\":5,\"cacheWrite\":0,\"cost\":{\"total\":0.0012}}}}\n{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n";
        let u = parse_pi_usage(pi);
        assert_eq!((u.input_tokens, u.output_tokens, u.turns), (100, 20, 1));
        assert_eq!(u.cost_source, CostSource::Reported);
        assert!((u.cost_usd.unwrap() - 0.0012).abs() < 1e-12);

        let oc = "{\"id\":\"msg_1\",\"role\":\"user\"}\n{\"id\":\"msg_2\",\"role\":\"assistant\",\"modelID\":\"gpt-5\",\"tokens\":{\"input\":300,\"output\":40,\"reasoning\":10,\"cache\":{\"read\":200,\"write\":0}},\"cost\":0.003}\n";
        let u = parse_opencode_usage(oc);
        assert_eq!(
            (u.input_tokens, u.cache_read_tokens, u.reasoning_tokens),
            (300, 200, 10)
        );
        assert_eq!(u.models, vec!["gpt-5"]);
        assert_eq!(u.cost_source, CostSource::Reported);
    }

    #[test]
    fn usage_add_merges_cost_and_models() {
        let mut a = parse_pi_usage(
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"model\":\"m1\",\"usage\":{\"input\":1,\"output\":1,\"cost\":{\"total\":0.5}}}}\n",
        );
        let b = parse_pi_usage(
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"model\":\"m2\",\"usage\":{\"input\":2,\"output\":2,\"cost\":{\"total\":0.25}}}}\n",
        );
        a.add(&b);
        assert_eq!(a.total_tokens(), 6);
        assert!((a.cost_usd.unwrap() - 0.75).abs() < 1e-12);
        assert_eq!(a.models, vec!["m1", "m2"]);
    }
}
