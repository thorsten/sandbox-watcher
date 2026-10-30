//! Turn two consecutive overviews into user-facing notifications.
//!
//! Works for every runtime because it compares snapshots instead of relying
//! on the event stream, which Apple `container` does not have.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{AgentKind, NetworkStatus};
use crate::network::NetworkReport;
use crate::overview::{ContainerOverview, Overview};
use crate::settings::NotificationSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationKind {
    SessionStarted,
    SessionEnded,
    ContainerStarted,
    ContainerStopped,
    ContainerDied,
    BlockedBurst,
    StaleContainer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    pub container_id: String,
}

fn agent_label(agent: AgentKind, command: &str) -> String {
    match agent {
        AgentKind::Other => command
            .split_whitespace()
            .next()
            .unwrap_or("session")
            .to_string(),
        other => other.label().to_string(),
    }
}

fn by_id(overview: &Overview) -> HashMap<&str, &ContainerOverview> {
    overview
        .containers
        .iter()
        .map(|c| (c.container.id.as_str(), c))
        .collect()
}

/// Containers whose hash differs from the newest running container of their
/// project. Mirrors `staleHashes` in the UI.
pub fn stale_ids(overview: &Overview) -> HashSet<String> {
    let mut by_project: HashMap<&str, Vec<&ContainerOverview>> = HashMap::new();
    for c in &overview.containers {
        by_project
            .entry(c.container.project_slug.as_str())
            .or_default()
            .push(c);
    }
    let mut stale = HashSet::new();
    for group in by_project.values() {
        let hashes: HashSet<&str> = group
            .iter()
            .filter_map(|c| c.container.config_hash.as_deref())
            .collect();
        if hashes.len() <= 1 {
            continue;
        }
        let current = group
            .iter()
            .filter(|c| c.container.state.is_running() && c.container.started_at.is_some())
            .max_by_key(|c| c.container.started_at)
            .and_then(|c| c.container.config_hash.as_deref());
        for c in group {
            if let (Some(hash), Some(current)) = (c.container.config_hash.as_deref(), current)
                && hash != current
            {
                stale.insert(c.container.id.clone());
            }
        }
    }
    stale
}

/// Compare `previous` with `next` and produce notifications allowed by `settings`.
pub fn diff(
    previous: &Overview,
    next: &Overview,
    settings: &NotificationSettings,
) -> Vec<Notification> {
    let mut out = Vec::new();
    if !settings.enabled {
        return out;
    }
    let before = by_id(previous);
    let after = by_id(next);

    for (id, now) in &after {
        let name = &now.container.name;
        match before.get(id) {
            None => {
                if settings.container_started && now.container.state.is_running() {
                    out.push(Notification {
                        kind: NotificationKind::ContainerStarted,
                        title: format!("Sandbox started: {}", now.container.project_slug),
                        body: format!("{name} is running"),
                        container_id: (*id).to_string(),
                    });
                }
            }
            Some(was) => {
                let was_running = was.container.state.is_running();
                let is_running = now.container.state.is_running();
                if !was_running && is_running && settings.container_started {
                    out.push(Notification {
                        kind: NotificationKind::ContainerStarted,
                        title: format!("Sandbox started: {}", now.container.project_slug),
                        body: format!("{name} is running again"),
                        container_id: (*id).to_string(),
                    });
                }
                if was_running && !is_running {
                    // Sessions were active a moment ago: that was not an idle stop.
                    if !was.sessions.is_empty() && settings.container_died {
                        out.push(Notification {
                            kind: NotificationKind::ContainerDied,
                            title: format!(
                                "Sandbox stopped unexpectedly: {}",
                                now.container.project_slug
                            ),
                            body: format!(
                                "{name} stopped while {} session{} ran",
                                was.sessions.len(),
                                if was.sessions.len() == 1 { "" } else { "s" }
                            ),
                            container_id: (*id).to_string(),
                        });
                    } else if settings.container_stopped {
                        out.push(Notification {
                            kind: NotificationKind::ContainerStopped,
                            title: format!("Sandbox stopped: {}", now.container.project_slug),
                            body: format!("{name} is no longer running"),
                            container_id: (*id).to_string(),
                        });
                    }
                }
                if is_running {
                    let old: HashSet<u32> = was.sessions.iter().map(|s| s.pid).collect();
                    let new: HashSet<u32> = now.sessions.iter().map(|s| s.pid).collect();
                    if settings.session_started {
                        for s in now.sessions.iter().filter(|s| !old.contains(&s.pid)) {
                            out.push(Notification {
                                kind: NotificationKind::SessionStarted,
                                title: format!(
                                    "{} started in {}",
                                    agent_label(s.agent, &s.command),
                                    now.container.project_slug
                                ),
                                body: format!("{name}, PID {}", s.pid),
                                container_id: (*id).to_string(),
                            });
                        }
                    }
                    if settings.session_ended {
                        for s in was.sessions.iter().filter(|s| !new.contains(&s.pid)) {
                            out.push(Notification {
                                kind: NotificationKind::SessionEnded,
                                title: format!(
                                    "{} ended in {}",
                                    agent_label(s.agent, &s.command),
                                    now.container.project_slug
                                ),
                                body: format!("{name}, PID {}", s.pid),
                                container_id: (*id).to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    if settings.stale_container {
        let was_stale = stale_ids(previous);
        for id in stale_ids(next).difference(&was_stale) {
            if let Some(c) = after.get(id.as_str()) {
                out.push(Notification {
                    kind: NotificationKind::StaleContainer,
                    title: format!("Stale sandbox: {}", c.container.project_slug),
                    body: format!("{} was created from an older configuration than the newest container of this project", c.container.name),
                    container_id: id.clone(),
                });
            }
        }
    }
    out
}

/// Blocked requests per container seen within the window. Compare two of
/// these to detect a burst.
pub fn recent_blocked(
    reports: &[NetworkReport],
    now: DateTime<Utc>,
    window_secs: u64,
) -> HashMap<String, u64> {
    let since = now - Duration::seconds(window_secs as i64);
    let mut counts = HashMap::new();
    for report in reports {
        let n: u64 = report
            .entries
            .iter()
            .filter(|e| {
                e.status == NetworkStatus::Blocked && e.last_seen.is_some_and(|t| t >= since)
            })
            .map(|e| e.count)
            .sum();
        counts.insert(report.container_id.clone(), n);
    }
    counts
}

/// A burst notification when a container's recent blocked count crossed the
/// threshold since the last check.
pub fn blocked_bursts(
    previous: &HashMap<String, u64>,
    current: &HashMap<String, u64>,
    overview: &Overview,
    threshold: u64,
    settings: &NotificationSettings,
) -> Vec<Notification> {
    if !settings.enabled || !settings.blocked_burst {
        return vec![];
    }
    let names = by_id(overview);
    current
        .iter()
        .filter(|(id, n)| **n >= threshold && previous.get(*id).copied().unwrap_or(0) < threshold)
        .filter_map(|(id, n)| {
            let c = names.get(id.as_str())?;
            Some(Notification {
                kind: NotificationKind::BlockedBurst,
                title: format!("Blocked requests: {}", c.container.project_slug),
                body: format!("{n} blocked connections recently from {}", c.container.name),
                container_id: id.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeAdapter;
    use crate::mock::MockAdapter;
    use crate::model::{RuntimeKind, Session};
    use crate::overview::{DataSource, load};
    use std::sync::Arc;

    async fn snapshot(adapter: &MockAdapter) -> Overview {
        load(Arc::new(adapter.clone()), DataSource::Mock, vec![]).await
    }

    fn kinds(notifications: &[Notification]) -> Vec<NotificationKind> {
        notifications.iter().map(|n| n.kind).collect()
    }

    #[tokio::test]
    async fn detects_stops_with_and_without_sessions() {
        let mock = MockAdapter::demo();
        let before = snapshot(&mock).await;
        mock.stop("sandbox-web-shop", None).await.unwrap(); // had sessions
        mock.stop("sandbox-billing-api-live", None).await.unwrap(); // had sessions too (mock default)
        let after = snapshot(&mock).await;
        let n = diff(&before, &after, &NotificationSettings::default());
        assert_eq!(n.len(), 2);
        assert!(n.iter().all(|x| x.kind == NotificationKind::ContainerDied));

        // Idle stop: no sessions before.
        let mut before2 = after.clone();
        let mut after2 = after.clone();
        let idx = before2
            .containers
            .iter()
            .position(|c| c.container.name == "sandbox-web-shop-2")
            .unwrap();
        before2.containers[idx].sessions.clear();
        after2.containers[idx].sessions.clear();
        after2.containers[idx].container.state = crate::model::ContainerState::Exited;
        let n = diff(&before2, &after2, &NotificationSettings::default());
        assert_eq!(kinds(&n), vec![NotificationKind::ContainerStopped]);
    }

    #[tokio::test]
    async fn detects_new_sessions_and_new_containers() {
        let mock = MockAdapter::demo();
        let before = snapshot(&mock).await;
        let mut after = before.clone();
        after.containers[1].sessions.push(Session {
            pid: 9999,
            command: "codex".into(),
            agent: AgentKind::Codex,
        });
        let n = diff(&before, &after, &NotificationSettings::default());
        assert_eq!(kinds(&n), vec![NotificationKind::SessionStarted]);
        assert!(n[0].title.starts_with("Codex started in"));

        let muted = NotificationSettings {
            session_started: false,
            ..Default::default()
        };
        assert!(diff(&before, &after, &muted).is_empty());

        let mut brand_new = before.clone();
        let mut extra = before.containers[1].clone();
        extra.container.id = "new-id".into();
        extra.container.name = "sandbox-new".into();
        extra.container.project_slug = "new".into();
        extra.runtime = RuntimeKind::Docker;
        brand_new.containers.push(extra);
        let n = diff(&before, &brand_new, &NotificationSettings::default());
        assert!(
            n.iter()
                .any(|x| x.kind == NotificationKind::ContainerStarted && x.title.contains("new"))
        );
    }

    #[tokio::test]
    async fn stale_only_fires_once() {
        let mock = MockAdapter::demo();
        let mut before = snapshot(&mock).await;
        let i1 = before
            .containers
            .iter()
            .position(|c| c.container.name == "sandbox-web-shop")
            .unwrap();
        let i2 = before
            .containers
            .iter()
            .position(|c| c.container.name == "sandbox-web-shop-2")
            .unwrap();
        // Same hash on both: nothing is stale yet.
        before.containers[i2].container.config_hash =
            before.containers[i1].container.config_hash.clone();
        let mut after = before.clone();
        after.containers[i2].container.config_hash = Some("ffffffffffff".into());
        after.containers[i2].container.started_at = Some(Utc::now());
        let n = diff(&before, &after, &NotificationSettings::default());
        let stale: Vec<_> = n
            .iter()
            .filter(|x| x.kind == NotificationKind::StaleContainer)
            .collect();
        assert_eq!(stale.len(), 1, "{n:?}");
        assert!(
            stale[0].body.starts_with("sandbox-web-shop "),
            "the older container is stale"
        );
        let again = diff(&after, &after, &NotificationSettings::default());
        assert!(again.is_empty());
    }

    #[tokio::test]
    async fn blocked_burst_crosses_threshold_once() {
        let mock = MockAdapter::demo();
        let overview = snapshot(&mock).await;
        let id = overview.containers[1].container.id.clone();
        let prev = HashMap::from([(id.clone(), 2u64)]);
        let cur = HashMap::from([(id.clone(), 7u64)]);
        let n = blocked_bursts(&prev, &cur, &overview, 5, &NotificationSettings::default());
        assert_eq!(kinds(&n), vec![NotificationKind::BlockedBurst]);
        let n = blocked_bursts(&cur, &cur, &overview, 5, &NotificationSettings::default());
        assert!(n.is_empty(), "stays above threshold: no repeat");
    }
}
