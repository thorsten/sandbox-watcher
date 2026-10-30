//! Network activity of a sandbox container, read from the logs sandbox keeps
//! inside it and reported through `sandbox-container-tools network diagnostic`.
//!
//! Parsers are ports of sandbox 0.73.1's `network-diagnostics` module and
//! stay line-compatible with it. See `fixtures/sandbox-logs/README.md`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::adapter::{ExecRequest, RuntimeAdapter};
use crate::model::{NetworkEntry, NetworkStatus, SandboxContainer};
use crate::sandbox::CONTAINER_TOOLS_PATH;

/// Log files sandbox exposes via `network diagnostic <source>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkSource {
    Firewall,
    Dns,
    ProxyAccess,
}

impl NetworkSource {
    pub fn as_str(self) -> &'static str {
        match self {
            NetworkSource::Firewall => "firewall",
            NetworkSource::Dns => "dns",
            NetworkSource::ProxyAccess => "proxy-access",
        }
    }

    /// Command that prints the log. Must run as root inside the container.
    pub fn command(self) -> Vec<String> {
        vec![
            CONTAINER_TOOLS_PATH.into(),
            "network".into(),
            "diagnostic".into(),
            self.as_str().into(),
        ]
    }
}

/// Prefix shared by every diagnostic exec, used to filter our own events.
pub const DIAGNOSTIC_COMMAND_PREFIX: &str = "sandbox-container-tools network diagnostic";

/// Network entries of one container plus notes about collection quality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkReport {
    pub container_id: String,
    pub collected_at: DateTime<Utc>,
    pub entries: Vec<NetworkEntry>,
    /// Sources that returned content none of our parsers understood. A sign
    /// that the sandbox release in the container is newer than we tested.
    pub unparsed_sources: Vec<NetworkSource>,
    /// Sources whose exec failed (container tools missing, exec denied).
    pub failed_sources: Vec<NetworkSource>,
}

// ---------------------------------------------------------------------------
// Firewall (tcpdump) log
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct BlockedRequest {
    pub at: DateTime<Utc>,
    pub dst_ip: String,
    pub dst_port: u16,
}

fn split_ip_port(token: &str) -> Option<(&str, u16)> {
    let (ip, port) = token.rsplit_once('.')?;
    let port = port.parse().ok()?;
    if ip.split('.').count() != 4 || !ip.split('.').all(|o| o.parse::<u8>().is_ok()) {
        return None;
    }
    Some((ip, port))
}

fn epoch_seconds(value: &str) -> Option<DateTime<Utc>> {
    let secs: f64 = value.parse().ok()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Utc.timestamp_millis_opt((secs * 1000.0).round() as i64)
        .single()
}

/// Parse tcpdump IPv4 lines: `<epoch> IP <src>.<port> > <dst>.<port>: ...`.
/// IPv6 lines are skipped, as in sandbox.
pub fn parse_firewall_log(raw: &str) -> Vec<BlockedRequest> {
    raw.lines()
        .filter_map(|line| {
            let mut t = line.split_whitespace();
            let at = epoch_seconds(t.next()?)?;
            if t.next()? != "IP" {
                return None;
            }
            split_ip_port(t.next()?)?;
            if t.next()? != ">" {
                return None;
            }
            let dst = t.next()?.trim_end_matches(':');
            let (dst_ip, dst_port) = split_ip_port(dst)?;
            Some(BlockedRequest {
                at,
                dst_ip: dst_ip.to_string(),
                dst_port,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Proxy (squid) log
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyRequest {
    pub at: DateTime<Utc>,
    pub domain: String,
    pub port: u16,
    pub status: NetworkStatus,
}

fn domain_port(method: &str, url: &str) -> Option<(String, u16)> {
    if method == "CONNECT" {
        let (host, port) = url.rsplit_once(':')?;
        return Some((host.to_ascii_lowercase(), port.parse().ok()?));
    }
    let (scheme, rest) = url.split_once("://")?;
    let default_port = match scheme {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    let authority = rest.split('/').next()?;
    let authority = authority.rsplit('@').next()?;
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(']') || host.ends_with(']') => Some((
            host.to_ascii_lowercase(),
            port.parse().unwrap_or(default_port),
        )),
        _ => Some((authority.to_ascii_lowercase(), default_port)),
    }
}

/// Parse squid native access log lines.
pub fn parse_proxy_log(raw: &str) -> Vec<ProxyRequest> {
    raw.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 7 {
                return None;
            }
            let at = epoch_seconds(f[0])?;
            let squid_status = f[3].split('/').next().unwrap_or("");
            let (domain, port) = domain_port(f[5], f[6])?;
            let status = if squid_status == "TCP_DENIED" {
                NetworkStatus::Blocked
            } else {
                NetworkStatus::Allowed
            };
            Some(ProxyRequest {
                at,
                domain,
                port,
                status,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// DNS (dnsmasq) log
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct DnsLine {
    pub at: DateTime<Utc>,
    pub domain: String,
    /// `reply X is <value>`: the value. `config X is <value>`: the value.
    pub value: String,
    pub blocked: bool,
}

/// Parse a syslog timestamp like `Feb 16 14:11:39` using the reference year.
/// Dates more than a day in the future roll back one year.
pub fn parse_syslog_timestamp(fields: &[&str], reference: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let [month, day, time] = fields else {
        return None;
    };
    let parse = |year: i32| {
        NaiveDateTime::parse_from_str(&format!("{year} {month} {day} {time}"), "%Y %b %d %H:%M:%S")
            .ok()
            .map(|naive| Utc.from_utc_datetime(&naive))
    };
    let candidate = parse(reference.year())?;
    if candidate > reference + Duration::days(1) {
        parse(reference.year() - 1)
    } else {
        Some(candidate)
    }
}

/// Parse dnsmasq `reply` and `config` lines.
pub fn parse_dns_log(raw: &str, reference: DateTime<Utc>) -> Vec<DnsLine> {
    raw.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // Mon DD HH:MM:SS dnsmasq[pid]: <verb> <domain> is <value>
            let verb_index = fields
                .iter()
                .position(|f| *f == "reply" || *f == "config")?;
            let domain = fields.get(verb_index + 1)?;
            if fields.get(verb_index + 2).copied() != Some("is") {
                return None;
            }
            let value = fields.get(verb_index + 3)?;
            let at = parse_syslog_timestamp(&fields[..fields.len().min(3)], reference)
                .unwrap_or(reference);
            Some(DnsLine {
                at,
                domain: domain.to_ascii_lowercase(),
                value: value.to_string(),
                blocked: fields[verb_index] == "config",
            })
        })
        .collect()
}

fn is_ip(value: &str) -> bool {
    value.parse::<std::net::IpAddr>().is_ok()
}

/// Most recent domain seen for each IP.
pub fn ip_to_domain(lines: &[DnsLine]) -> HashMap<String, String> {
    let mut latest: HashMap<String, (DateTime<Utc>, String)> = HashMap::new();
    for line in lines.iter().filter(|l| !l.blocked && is_ip(&l.value)) {
        let entry = latest
            .entry(line.value.clone())
            .or_insert_with(|| (line.at, line.domain.clone()));
        if line.at >= entry.0 {
            *entry = (line.at, line.domain.clone());
        }
    }
    latest
        .into_iter()
        .map(|(ip, (_, domain))| (ip, domain))
        .collect()
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Agg {
    count: u64,
    last_seen: Option<DateTime<Utc>>,
}

impl Agg {
    fn add(&mut self, at: DateTime<Utc>) {
        self.count += 1;
        self.last_seen = Some(self.last_seen.map_or(at, |s| s.max(at)));
    }
}

/// Combine the three logs into entries the way `sandbox network logs` does:
/// firewall hits (resolved to domains via DNS replies when possible), proxy
/// requests, then DNS-only domains not already covered by the proxy.
pub fn build_entries(
    container_id: &str,
    firewall: &[BlockedRequest],
    proxy: &[ProxyRequest],
    dns: &[DnsLine],
) -> Vec<NetworkEntry> {
    let names = ip_to_domain(dns);
    let mut entries = Vec::new();

    let mut fw: BTreeMap<(String, u16), Agg> = BTreeMap::new();
    for r in firewall {
        fw.entry((r.dst_ip.clone(), r.dst_port))
            .or_default()
            .add(r.at);
    }
    for ((ip, port), agg) in fw {
        entries.push(NetworkEntry {
            container_id: container_id.into(),
            destination: names.get(&ip).cloned().unwrap_or_else(|| ip.clone()),
            dst_ip: Some(ip),
            port,
            count: agg.count,
            last_seen: agg.last_seen,
            status: NetworkStatus::Blocked,
        });
    }

    let mut px: BTreeMap<(String, u16, NetworkStatus), Agg> = BTreeMap::new();
    for r in proxy {
        px.entry((r.domain.clone(), r.port, r.status))
            .or_default()
            .add(r.at);
    }
    let proxy_domains: std::collections::HashSet<&str> =
        proxy.iter().map(|r| r.domain.as_str()).collect();
    let proxy_blocked: std::collections::HashSet<&str> = proxy
        .iter()
        .filter(|r| r.status == NetworkStatus::Blocked)
        .map(|r| r.domain.as_str())
        .collect();
    for ((domain, port, status), agg) in px {
        entries.push(NetworkEntry {
            container_id: container_id.into(),
            destination: domain,
            dst_ip: None,
            port,
            count: agg.count,
            last_seen: agg.last_seen,
            status,
        });
    }

    let mut dns_blocked: BTreeMap<String, Agg> = BTreeMap::new();
    let mut dns_allowed: BTreeMap<String, Agg> = BTreeMap::new();
    for line in dns {
        if line.blocked {
            dns_blocked
                .entry(line.domain.clone())
                .or_default()
                .add(line.at);
        } else if is_ip(&line.value) {
            dns_allowed
                .entry(line.domain.clone())
                .or_default()
                .add(line.at);
        }
    }
    for (domain, agg) in dns_blocked {
        if proxy_blocked.contains(domain.as_str()) {
            continue;
        }
        entries.push(NetworkEntry {
            container_id: container_id.into(),
            destination: domain,
            dst_ip: None,
            port: 0,
            count: agg.count,
            last_seen: agg.last_seen,
            status: NetworkStatus::Blocked,
        });
    }
    for (domain, agg) in dns_allowed {
        if proxy_domains.contains(domain.as_str()) {
            continue;
        }
        entries.push(NetworkEntry {
            container_id: container_id.into(),
            destination: domain,
            dst_ip: None,
            port: 0,
            count: agg.count,
            last_seen: agg.last_seen,
            status: NetworkStatus::Allowed,
        });
    }

    entries.sort_by(|a, b| {
        (b.status == NetworkStatus::Blocked)
            .cmp(&(a.status == NetworkStatus::Blocked))
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.destination.cmp(&b.destination))
    });
    entries
}

/// Does this raw log contain anything other than blank lines and sandbox's
/// "nothing recorded" placeholders?
fn has_content(raw: &str) -> bool {
    raw.lines().map(str::trim).any(|l| {
        !l.is_empty()
            && !l.starts_with("No blocked requests recorded")
            && !l.starts_with("Proxy log file not found")
    })
}

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

async fn read_source(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
    source: NetworkSource,
) -> Result<String> {
    let output = adapter
        .exec(container_id, ExecRequest::new(source.command()).as_root())
        .await?;
    if output.exit_code != 0 {
        return Err(crate::Error::Protocol(format!(
            "network diagnostic {} exited with {}: {}",
            source.as_str(),
            output.exit_code,
            output.stderr.trim()
        )));
    }
    Ok(output.stdout)
}

/// Read and combine the network logs of one running container.
pub async fn collect(
    adapter: &Arc<dyn RuntimeAdapter>,
    container: &SandboxContainer,
    now: DateTime<Utc>,
) -> NetworkReport {
    let id = container.id.as_str();
    let (firewall_raw, dns_raw, proxy_raw) = futures::join!(
        read_source(adapter, id, NetworkSource::Firewall),
        read_source(adapter, id, NetworkSource::Dns),
        read_source(adapter, id, NetworkSource::ProxyAccess),
    );

    let mut failed = Vec::new();
    let mut unparsed = Vec::new();
    let mut take = |source: NetworkSource, raw: Result<String>| match raw {
        Ok(raw) => raw,
        Err(_) => {
            failed.push(source);
            String::new()
        }
    };
    let firewall_raw = take(NetworkSource::Firewall, firewall_raw);
    let dns_raw = take(NetworkSource::Dns, dns_raw);
    let proxy_raw = take(NetworkSource::ProxyAccess, proxy_raw);

    let firewall = parse_firewall_log(&firewall_raw);
    let proxy = parse_proxy_log(&proxy_raw);
    let dns = parse_dns_log(&dns_raw, now);
    if has_content(&firewall_raw) && firewall.is_empty() {
        unparsed.push(NetworkSource::Firewall);
    }
    if has_content(&proxy_raw) && proxy.is_empty() {
        unparsed.push(NetworkSource::ProxyAccess);
    }
    // dnsmasq logs plenty of lines that are not replies; only flag it when
    // there is content but no query/reply vocabulary at all.
    if has_content(&dns_raw) && dns.is_empty() && !dns_raw.contains("dnsmasq") {
        unparsed.push(NetworkSource::Dns);
    }

    NetworkReport {
        container_id: id.to_string(),
        collected_at: now,
        entries: build_entries(id, &firewall, &proxy, &dns),
        unparsed_sources: unparsed,
        failed_sources: failed,
    }
}

/// Collect reports for every running container, concurrently.
pub async fn collect_all(
    adapter: &Arc<dyn RuntimeAdapter>,
    containers: &[SandboxContainer],
    now: DateTime<Utc>,
) -> Vec<NetworkReport> {
    let running = containers.iter().filter(|c| c.state.is_running());
    futures::future::join_all(running.map(|c| collect(adapter, c, now))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap()
    }

    #[test]
    fn syslog_timestamps_use_reference_year_and_roll_back() {
        let at = parse_syslog_timestamp(&["Oct", "30", "08:00:05"], reference()).unwrap();
        assert_eq!(at.to_rfc3339(), "2026-10-30T08:00:05+00:00");
        // December in a January log belongs to the previous year.
        let jan = Utc.with_ymd_and_hms(2027, 1, 2, 0, 0, 0).unwrap();
        let at = parse_syslog_timestamp(&["Dec", "31", "23:00:00"], jan).unwrap();
        assert_eq!(at.year(), 2026);
        assert!(parse_syslog_timestamp(&["nope"], reference()).is_none());
    }

    #[test]
    fn proxy_url_parsing_matches_sandbox() {
        assert_eq!(
            domain_port("CONNECT", "Registry.NPMJS.org:443"),
            Some(("registry.npmjs.org".into(), 443))
        );
        assert_eq!(
            domain_port("GET", "http://example.com/path"),
            Some(("example.com".into(), 80))
        );
        assert_eq!(
            domain_port("GET", "https://tracker.example:8443/pixel"),
            Some(("tracker.example".into(), 8443))
        );
        assert_eq!(domain_port("GET", "ftp://x/y"), None);
        assert_eq!(domain_port("CONNECT", "noport"), None);
    }

    #[test]
    fn firewall_lines_need_ipv4_shape() {
        assert!(parse_firewall_log("1.0 IP6 ::1.1 > ::2.2: x").is_empty());
        assert!(parse_firewall_log("garbage").is_empty());
        let r =
            parse_firewall_log("1761810001.5 IP 172.17.0.2.51234 > 93.184.216.34.443: Flags [S]");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].dst_port, 443);
        assert_eq!(r[0].at.timestamp_millis(), 1_761_810_001_500);
    }

    #[test]
    fn has_content_ignores_placeholders() {
        assert!(!has_content("No blocked requests recorded.\n"));
        assert!(!has_content("\n\n"));
        assert!(has_content("something\n"));
    }
}

#[cfg(test)]
mod collect_tests {
    use super::*;
    use crate::adapter::ContainerFilter;
    use crate::mock::{MockAdapter, mock_container};

    #[tokio::test]
    async fn collects_from_mock_container() {
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let containers = adapter
            .list_containers(&ContainerFilter::all())
            .await
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap();
        let reports = collect_all(&adapter, &containers, now).await;
        assert_eq!(reports.len(), 3, "one report per running container");
        let report = &reports[0];
        assert!(report.failed_sources.is_empty());
        assert!(report.unparsed_sources.is_empty());
        let blocked: Vec<_> = report
            .entries
            .iter()
            .filter(|e| e.status == NetworkStatus::Blocked)
            .map(|e| e.destination.as_str())
            .collect();
        assert_eq!(
            blocked,
            vec!["example.com", "evil.example", "telemetry.example"]
        );
    }

    #[tokio::test]
    async fn flags_unparsed_sources() {
        let adapter = MockAdapter::empty();
        let mut c = mock_container("odd", 1, "running", "abc");
        c.network_logs.insert(
            NetworkSource::ProxyAccess,
            "totally new format v99\n".into(),
        );
        c.network_logs.insert(
            NetworkSource::Firewall,
            "No blocked requests recorded.\n".into(),
        );
        adapter.add(c);
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(adapter);
        let containers = adapter
            .list_containers(&ContainerFilter::all())
            .await
            .unwrap();
        let report = collect(&adapter, &containers[0], Utc::now()).await;
        assert_eq!(report.unparsed_sources, vec![NetworkSource::ProxyAccess]);
    }
}
