//! Local history of sandbox containers and agent sessions, derived from
//! consecutive overviews. Stored as one JSON file; sizes stay small because
//! records are pruned after a retention period.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::files::FileTouch;
use crate::host::{Host, parse_container_key};
use crate::model::{AgentKind, RuntimeKind};
use crate::overview::Overview;
use crate::usage::Usage;

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub container_id: String,
    pub container_name: String,
    pub project_slug: String,
    pub runtime: RuntimeKind,
    pub agent: AgentKind,
    pub command: String,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_seen_at: DateTime<Utc>,
    /// Last usage snapshot for this session's agent transcript.
    pub usage: Option<Usage>,
    /// Files read and changed, from the agent's transcript.
    #[serde(default)]
    pub files: Vec<FileTouch>,
}

impl SessionRecord {
    pub fn duration_secs(&self, now: DateTime<Utc>) -> i64 {
        (self.ended_at.unwrap_or(now) - self.started_at)
            .num_seconds()
            .max(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerRecord {
    pub container_id: String,
    pub name: String,
    pub project_slug: String,
    pub runtime: RuntimeKind,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub version: u32,
    pub sessions: Vec<SessionRecord>,
    pub containers: Vec<ContainerRecord>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            sessions: vec![],
            containers: vec![],
        }
    }
}

impl History {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<History>(&text).ok())
            .filter(|h| h.version == FORMAT_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string(self).unwrap_or_else(|_| "{}".into()),
        )?;
        std::fs::rename(&tmp, path)
    }

    /// Records left open by a previous run (crash, quit) end at their last sighting.
    pub fn close_stale(&mut self) -> bool {
        let mut changed = false;
        for s in self.sessions.iter_mut().filter(|s| s.ended_at.is_none()) {
            s.ended_at = Some(s.last_seen_at);
            changed = true;
        }
        for c in self.containers.iter_mut().filter(|c| c.ended_at.is_none()) {
            c.ended_at = Some(c.last_seen_at);
            changed = true;
        }
        changed
    }

    /// Fold a fresh overview in. Returns whether anything changed besides
    /// `last_seen_at` timestamps (callers may save less often otherwise).
    pub fn observe(&mut self, overview: &Overview, now: DateTime<Utc>) -> bool {
        let mut changed = false;
        let mut seen_sessions: HashMap<(String, u32), ()> = HashMap::new();
        let mut seen_containers: HashMap<String, ()> = HashMap::new();
        // Hosts shown from their last snapshot: nothing is known about them
        // this round, so their open records stay open and unbumped.
        let unreachable: HashSet<Host> = overview
            .containers
            .iter()
            .filter(|c| c.last_seen_at.is_some())
            .map(|c| c.host.clone())
            .collect();
        let on_unreachable = |key: &str| unreachable.contains(&parse_container_key(key).0);

        for c in overview
            .containers
            .iter()
            .filter(|c| c.container.state.is_running() && c.last_seen_at.is_none())
        {
            let id = c.key.clone();
            seen_containers.insert(id.clone(), ());
            match self
                .containers
                .iter_mut()
                .find(|r| r.container_id == id && r.ended_at.is_none())
            {
                Some(record) => record.last_seen_at = now,
                None => {
                    self.containers.push(ContainerRecord {
                        container_id: id.clone(),
                        name: c.container.name.clone(),
                        project_slug: c.container.project_slug.clone(),
                        runtime: c.runtime,
                        started_at: c.container.started_at.unwrap_or(now),
                        ended_at: None,
                        last_seen_at: now,
                    });
                    changed = true;
                }
            }
            for s in &c.sessions {
                seen_sessions.insert((id.clone(), s.pid), ());
                match self
                    .sessions
                    .iter_mut()
                    .find(|r| r.container_id == id && r.pid == s.pid && r.ended_at.is_none())
                {
                    Some(record) => record.last_seen_at = now,
                    None => {
                        self.sessions.push(SessionRecord {
                            id: format!("{}:{}:{}", id, s.pid, now.timestamp()),
                            container_id: id.clone(),
                            container_name: c.container.name.clone(),
                            project_slug: c.container.project_slug.clone(),
                            runtime: c.runtime,
                            agent: s.agent,
                            command: s.command.clone(),
                            pid: s.pid,
                            started_at: now,
                            ended_at: None,
                            last_seen_at: now,
                            usage: None,
                            files: vec![],
                        });
                        changed = true;
                    }
                }
            }
        }

        for s in self.sessions.iter_mut().filter(|s| s.ended_at.is_none()) {
            if !seen_sessions.contains_key(&(s.container_id.clone(), s.pid))
                && !on_unreachable(&s.container_id)
            {
                s.ended_at = Some(now);
                changed = true;
            }
        }
        for c in self.containers.iter_mut().filter(|c| c.ended_at.is_none()) {
            if !seen_containers.contains_key(&c.container_id) && !on_unreachable(&c.container_id) {
                c.ended_at = Some(now);
                changed = true;
            }
        }
        changed
    }

    /// Attach a usage snapshot to the newest open session of this agent in
    /// the container. Returns whether it changed.
    pub fn record_usage(&mut self, container_id: &str, usage: &Usage) -> bool {
        let Some(record) = self
            .sessions
            .iter_mut()
            .filter(|s| {
                s.container_id == container_id && s.agent == usage.agent && s.ended_at.is_none()
            })
            .max_by_key(|s| s.started_at)
        else {
            return false;
        };
        if record.usage.as_ref() == Some(usage) {
            return false;
        }
        record.usage = Some(usage.clone());
        true
    }

    /// Attach the touched files to the newest open session of this agent.
    pub fn record_files(
        &mut self,
        container_id: &str,
        agent: crate::model::AgentKind,
        files: &[FileTouch],
    ) -> bool {
        let Some(record) = self
            .sessions
            .iter_mut()
            .filter(|s| s.container_id == container_id && s.agent == agent && s.ended_at.is_none())
            .max_by_key(|s| s.started_at)
        else {
            return false;
        };
        if record.files == files {
            return false;
        }
        record.files = files.to_vec();
        true
    }

    /// Drop closed records older than the retention window.
    pub fn prune(&mut self, retention_days: u64, now: DateTime<Utc>) -> bool {
        let cutoff = now - Duration::days(retention_days as i64);
        let before = self.sessions.len() + self.containers.len();
        self.sessions
            .retain(|s| s.ended_at.is_none_or(|e| e >= cutoff));
        self.containers
            .retain(|c| c.ended_at.is_none_or(|e| e >= cutoff));
        before != self.sessions.len() + self.containers.len()
    }

    pub fn query(&self, query: &HistoryQuery, now: DateTime<Utc>) -> HistoryView {
        let mut sessions: Vec<SessionRecord> = self
            .sessions
            .iter()
            .filter(|s| s.ended_at.unwrap_or(now) >= query.since)
            .filter(|s| query.project.as_deref().is_none_or(|p| p == s.project_slug))
            .filter(|s| query.agent.is_none_or(|a| a == s.agent))
            .cloned()
            .collect();
        sessions.sort_by_key(|a| std::cmp::Reverse(a.started_at));

        let aggregate = |key_of: &dyn Fn(&SessionRecord) -> String| -> Vec<Aggregate> {
            let mut map: HashMap<String, Aggregate> = HashMap::new();
            for s in &sessions {
                let entry = map
                    .entry(key_of(s))
                    .or_insert_with(|| Aggregate::new(key_of(s)));
                entry.absorb(s, now);
            }
            let mut list: Vec<Aggregate> = map.into_values().collect();
            list.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.key.cmp(&b.key)));
            list
        };
        let by_agent = aggregate(&|s| s.agent.label().to_string());
        let by_project = aggregate(&|s| s.project_slug.clone());
        let mut total = Aggregate::new("total".into());
        for s in &sessions {
            total.absorb(s, now);
        }
        let projects: Vec<String> = {
            let mut p: Vec<String> = self
                .sessions
                .iter()
                .map(|s| s.project_slug.clone())
                .collect();
            p.sort();
            p.dedup();
            p
        };
        HistoryView {
            sessions,
            by_agent,
            by_project,
            total,
            projects,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub since: DateTime<Utc>,
    pub project: Option<String>,
    pub agent: Option<AgentKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Aggregate {
    pub key: String,
    pub sessions: u64,
    pub seconds: i64,
    pub tokens: u64,
    pub cost_usd: Option<f64>,
    /// True when at least one session had tokens but no price.
    pub cost_incomplete: bool,
}

impl Aggregate {
    fn new(key: String) -> Self {
        Self {
            key,
            sessions: 0,
            seconds: 0,
            tokens: 0,
            cost_usd: None,
            cost_incomplete: false,
        }
    }

    fn absorb(&mut self, s: &SessionRecord, now: DateTime<Utc>) {
        self.sessions += 1;
        self.seconds += s.duration_secs(now);
        if let Some(u) = &s.usage {
            self.tokens += u.total_tokens();
            match u.cost_usd {
                Some(c) => self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + c),
                None if u.total_tokens() > 0 => self.cost_incomplete = true,
                None => {}
            }
            if u.cost_source == crate::usage::CostSource::Unknown && u.total_tokens() > 0 {
                self.cost_incomplete = true;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryView {
    pub sessions: Vec<SessionRecord>,
    pub by_agent: Vec<Aggregate>,
    pub by_project: Vec<Aggregate>,
    pub total: Aggregate,
    /// Every project slug ever seen, for the filter dropdown.
    pub projects: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;

    use super::*;
    use crate::RuntimeAdapter;
    use crate::mock::MockAdapter;
    use crate::overview::{DataSource, load};

    async fn snapshot(mock: &MockAdapter) -> Overview {
        load(Arc::new(mock.clone()), DataSource::Mock, vec![]).await
    }

    #[tokio::test]
    async fn tracks_sessions_and_containers_across_overviews() {
        let mock = MockAdapter::demo();
        let t0 = Utc.with_ymd_and_hms(2026, 10, 30, 10, 0, 0).unwrap();
        let mut h = History::default();
        let o1 = snapshot(&mock).await;
        assert!(h.observe(&o1, t0));
        assert_eq!(
            h.containers.iter().filter(|c| c.ended_at.is_none()).count(),
            3
        );
        assert_eq!(
            h.sessions.iter().filter(|s| s.ended_at.is_none()).count(),
            6
        );

        // Same picture a bit later: nothing new, only sightings.
        assert!(!h.observe(&o1, t0 + Duration::seconds(10)));

        mock.stop("sandbox-web-shop", None).await.unwrap();
        let o2 = snapshot(&mock).await;
        let t1 = t0 + Duration::seconds(60);
        assert!(h.observe(&o2, t1));
        let ended: Vec<_> = h.sessions.iter().filter(|s| s.ended_at.is_some()).collect();
        assert_eq!(ended.len(), 2);
        assert!(ended.iter().all(|s| s.container_name == "sandbox-web-shop"));
        assert_eq!(ended[0].duration_secs(t1), 60);

        let view = h.query(
            &HistoryQuery {
                since: t0,
                project: None,
                agent: None,
            },
            t1,
        );
        assert_eq!(view.sessions.len(), 6);
        assert_eq!(view.total.sessions, 6);
        assert!(
            view.by_agent
                .iter()
                .any(|a| a.key == "Claude Code" && a.sessions == 3)
        );
        assert_eq!(view.projects, vec!["billing-api-live", "web-shop"]);

        let only_shell = h.query(
            &HistoryQuery {
                since: t0,
                project: Some("web-shop".into()),
                agent: Some(AgentKind::Shell),
            },
            t1,
        );
        assert_eq!(only_shell.sessions.len(), 2);
    }

    #[tokio::test]
    async fn close_stale_prune_and_persist() {
        let mock = MockAdapter::demo();
        let t0 = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();
        let mut h = History::default();
        h.observe(&snapshot(&mock).await, t0);
        assert!(h.close_stale());
        assert!(h.sessions.iter().all(|s| s.ended_at == Some(t0)));
        let now = t0 + Duration::days(40);
        assert!(h.prune(30, now));
        assert!(h.sessions.is_empty() && h.containers.is_empty());

        let dir = std::env::temp_dir().join(format!("sw-history-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h2 = History::default();
        h2.observe(&snapshot(&mock).await, t0);
        h2.save(&path).unwrap();
        let loaded = History::load(&path);
        assert_eq!(loaded, h2);
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(
            History::load(Path::new("/nonexistent/history.json")),
            History::default()
        );
    }

    #[tokio::test]
    async fn usage_attaches_to_the_newest_open_session() {
        let mock = MockAdapter::demo();
        let t0 = Utc.with_ymd_and_hms(2026, 10, 30, 10, 0, 0).unwrap();
        let mut h = History::default();
        let o = snapshot(&mock).await;
        h.observe(&o, t0);
        let shop = o
            .containers
            .iter()
            .find(|c| c.container.name == "sandbox-web-shop")
            .unwrap();
        let usage = crate::usage::parse_pi_usage("");
        let mut usage = Usage {
            agent: AgentKind::ClaudeCode,
            ..usage
        };
        usage.input_tokens = 42;
        assert!(h.record_usage(&shop.container.id, &usage));
        assert!(
            !h.record_usage(&shop.container.id, &usage),
            "unchanged snapshot is a no-op"
        );
        let view = h.query(
            &HistoryQuery {
                since: t0,
                project: None,
                agent: None,
            },
            t0,
        );
        assert_eq!(view.total.tokens, 42);
        assert!(view.total.cost_incomplete, "tokens without a price");
    }

    #[tokio::test]
    async fn remote_records_use_the_container_key() {
        let mock = MockAdapter::demo();
        let mut o = snapshot(&mock).await;
        for c in &mut o.containers {
            c.host = crate::host::Host::Remote("devbox".into());
            c.key = format!("devbox/{}", c.container.id);
        }
        let mut h = History::default();
        h.observe(&o, Utc.with_ymd_and_hms(2026, 10, 30, 10, 0, 0).unwrap());
        assert!(
            h.containers
                .iter()
                .all(|r| r.container_id.starts_with("devbox/"))
        );
        assert!(
            h.sessions
                .iter()
                .all(|r| r.container_id.starts_with("devbox/"))
        );
    }

    #[test]
    fn history_from_0_5_loads_and_keys_stay_ids() {
        let old = r#"{"version":1,"containers":[{"container_id":"abc","name":"sandbox-x","project_slug":"x","runtime":"docker","started_at":"2026-10-30T09:00:00Z","ended_at":null,"last_seen_at":"2026-10-30T10:00:00Z"}],"sessions":[]}"#;
        let history: History = serde_json::from_str(old).expect("0.5 history loads");
        assert_eq!(history.containers[0].container_id, "abc");
    }

    #[tokio::test]
    async fn unreachable_host_keeps_history_open_and_unbumped() {
        let mut live = snapshot(&MockAdapter::demo()).await;
        live.containers.retain(|c| !c.sessions.is_empty());
        live.containers.truncate(1);
        let host = crate::host::Host::Remote("devbox".into());
        for c in &mut live.containers {
            c.key = crate::host::container_key(&host, &c.container.id);
            c.host = host.clone();
        }
        let pids: Vec<u32> = live.containers[0].sessions.iter().map(|s| s.pid).collect();
        let t0 = Utc.with_ymd_and_hms(2026, 10, 30, 10, 0, 0).unwrap();
        let mut h = History::default();
        h.observe(&live, t0);

        let mut greyed = live.clone();
        greyed.containers = crate::remote_hosts::stale_snapshot(&live.containers, t0);
        let t1 = t0 + Duration::seconds(60);
        assert!(!h.observe(&greyed, t1), "an outage changes nothing");
        assert_eq!(
            h.containers[0].last_seen_at, t0,
            "not bumped while unreachable"
        );
        assert!(h.containers[0].ended_at.is_none());
        assert!(h.sessions.iter().all(|s| s.ended_at.is_none()));

        let t2 = t1 + Duration::seconds(60);
        h.observe(&live, t2);
        assert_eq!(h.sessions.len(), pids.len(), "one record per session");
        assert!(h.sessions.iter().all(|s| s.ended_at.is_none()));
        assert_eq!(h.containers.len(), 1);
        assert_eq!(h.containers[0].last_seen_at, t2);
    }
}
