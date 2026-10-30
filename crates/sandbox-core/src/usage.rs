//! Token usage and cost per agent session, read from the agents' own
//! transcripts inside the container.
//!
//! Pi and OpenCode report cost themselves. Claude Code and Codex report
//! tokens and a model; cost is estimated from a price table the user can
//! edit. Unknown models show tokens only.

use std::collections::BTreeMap;

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
pub fn default_prices() -> PriceTable {
    let p = |input, output, cache_read, cache_write| ModelPrice {
        input,
        output,
        cache_read,
        cache_write,
    };
    BTreeMap::from([
        ("claude-opus-4".into(), p(15.0, 75.0, 1.5, 18.75)),
        ("claude-sonnet-4".into(), p(3.0, 15.0, 0.3, 3.75)),
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
    fn new(agent: AgentKind) -> Self {
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
// ---------------------------------------------------------------------------

/// Extract only the usage and model fragments of a Claude transcript; the
/// file can be megabytes, the fragments are a few bytes per turn.
pub fn claude_usage_command(path: &str) -> Vec<String> {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    // Each assistant line yields: model|usage-json
    let script = format!(r#"grep -o '"model":"[^"]*"\|"usage":{{[^}}]*}}' {quoted} 2>/dev/null"#);
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

/// Parse the grep output: alternating `"model":"x"` and `"usage":{...}` fragments.
pub fn parse_claude_usage(output: &str, table: &PriceTable) -> Usage {
    let mut usage = Usage::new(AgentKind::ClaudeCode);
    let mut per_model: BTreeMap<String, Usage> = BTreeMap::new();
    let mut current_model = String::from("unknown");
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("\"model\":\"") {
            current_model = rest.trim_end_matches('"').to_string();
        } else if let Some(json) = line.strip_prefix("\"usage\":") {
            let Ok(v) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            let entry = per_model
                .entry(current_model.clone())
                .or_insert_with(|| Usage::new(AgentKind::ClaudeCode));
            let input = u64_of(v.get("input_tokens"));
            let output = u64_of(v.get("output_tokens"));
            let cr = u64_of(v.get("cache_read_input_tokens"));
            let cw = u64_of(v.get("cache_creation_input_tokens"));
            // Claude Code writes several assistant lines per turn (text, tool_use)
            // repeating the same usage; count a turn only when output tokens differ
            // from the previous fragment.
            entry.input_tokens += input;
            entry.output_tokens += output;
            entry.cache_read_tokens += cr;
            entry.cache_write_tokens += cw;
            entry.turns += 1;
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
    estimate(&mut usage, &per_model, table);
    usage
}

// ---------------------------------------------------------------------------
// Codex: token_count events carry cumulative totals in newer versions.
// ---------------------------------------------------------------------------

pub fn codex_usage_command(path: &str) -> Vec<String> {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    let script = format!(
        r#"grep -o '"model":"[^"]*"' {quoted} 2>/dev/null | head -1; grep '"token_count"' {quoted} 2>/dev/null | tail -1"#
    );
    vec!["sh".into(), "-c".into(), script, TRANSCRIPT_SENTINEL.into()]
}

pub fn parse_codex_usage(output: &str, table: &PriceTable) -> Usage {
    let mut usage = Usage::new(AgentKind::Codex);
    let mut model = String::from("unknown");
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("\"model\":\"") {
            model = rest.trim_end_matches('"').to_string();
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let payload = v.get("payload").unwrap_or(&v);
        // Newer: payload.info.total_token_usage; older: payload fields directly.
        let totals = payload
            .pointer("/info/total_token_usage")
            .or_else(|| payload.get("total_token_usage"))
            .unwrap_or(payload);
        usage.input_tokens = u64_of(totals.get("input_tokens"));
        usage.output_tokens = u64_of(totals.get("output_tokens"));
        usage.cache_read_tokens = u64_of(totals.get("cached_input_tokens"));
        usage.reasoning_tokens = u64_of(totals.get("reasoning_output_tokens"));
        usage.turns = 1;
    }
    // Codex reports cached tokens as part of input; price them separately.
    usage.input_tokens = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
    let mut per_model = BTreeMap::new();
    per_model.insert(model.clone(), usage.clone());
    usage.models = vec![model];
    estimate(&mut usage, &per_model, table);
    usage
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
    fn claude_usage_sums_per_model_and_estimates_cost() {
        let out = "\"model\":\"claude-sonnet-4-5-20250929\"\n\"usage\":{\"input_tokens\":1000,\"cache_creation_input_tokens\":2000,\"cache_read_input_tokens\":10000,\"output_tokens\":500}\n\"model\":\"claude-sonnet-4-5-20250929\"\n\"usage\":{\"input_tokens\":100,\"output_tokens\":50}\n";
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
            "\"model\":\"claude-future-9\"\n\"usage\":{\"input_tokens\":10,\"output_tokens\":1}\n",
            &default_prices(),
        );
        assert_eq!(unknown.cost_source, CostSource::Unknown);
        assert!(unknown.cost_usd.is_none());
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
