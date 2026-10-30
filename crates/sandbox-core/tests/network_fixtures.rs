//! Parse the sample logs in `fixtures/sandbox-logs/` the way sandbox does.

use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use sandbox_core::model::NetworkStatus;
use sandbox_core::network::{build_entries, parse_dns_log, parse_firewall_log, parse_proxy_log};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sandbox-logs")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn firewall_log_skips_ipv6_and_noise() {
    let requests = parse_firewall_log(&fixture("firewall-blocked.log"));
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2].dst_ip, "10.0.0.5");
    assert_eq!(requests[2].dst_port, 5432);
}

#[test]
fn proxy_log_marks_denied_and_derives_ports() {
    let requests = parse_proxy_log(&fixture("proxy-access.log"));
    assert_eq!(requests.len(), 5, "squid startup banner is skipped");
    assert_eq!(requests[1].status, NetworkStatus::Blocked);
    assert_eq!(requests[2].port, 80);
    assert_eq!(requests[4].port, 8443);
}

#[test]
fn dns_log_separates_blocked_and_replies() {
    let reference = Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap();
    let lines = parse_dns_log(&fixture("dns-proxy.log"), reference);
    let blocked: Vec<_> = lines.iter().filter(|l| l.blocked).collect();
    assert_eq!(blocked.len(), 2);
    assert!(blocked.iter().all(|l| l.domain == "evil.example"));
    let cname = lines.iter().find(|l| l.domain == "www.github.com").unwrap();
    assert_eq!(cname.value, "<CNAME>");
    let early = lines.iter().find(|l| l.domain == "early.example").unwrap();
    assert_eq!(
        early.at.format("%m-%d").to_string(),
        "10-02",
        "single-digit day with double space"
    );
}

#[test]
fn combined_entries_match_sandbox_network_logs() {
    let reference = Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap();
    let firewall = parse_firewall_log(&fixture("firewall-blocked.log"));
    let proxy = parse_proxy_log(&fixture("proxy-access.log"));
    let dns = parse_dns_log(&fixture("dns-proxy.log"), reference);
    let entries = build_entries("c1", &firewall, &proxy, &dns);

    let find = |dest: &str, port: u16| {
        entries
            .iter()
            .find(|e| e.destination == dest && e.port == port)
    };

    // Firewall hit resolved through the DNS reply for 93.184.216.34.
    let fw = find("example.com", 443).expect("firewall entry resolved to domain");
    assert_eq!(fw.status, NetworkStatus::Blocked);
    assert_eq!(fw.count, 2);
    assert_eq!(fw.dst_ip.as_deref(), Some("93.184.216.34"));
    // Unresolvable IP stays an IP.
    assert!(find("10.0.0.5", 5432).is_some());

    // Proxy entries aggregate by domain, port and status.
    assert_eq!(find("registry.npmjs.org", 443).unwrap().count, 2);
    assert_eq!(
        find("evil.example", 443).unwrap().status,
        NetworkStatus::Blocked
    );

    // DNS-blocked evil.example is already covered by the proxy denial: no port-0 duplicate.
    assert!(find("evil.example", 0).is_none());
    // DNS-only resolutions appear as allowed with port 0, unless the proxy saw the domain.
    assert!(find("github.com", 0).is_some());
    assert!(find("registry.npmjs.org", 0).is_none());
    // CNAME replies never count as resolutions.
    assert!(find("www.github.com", 0).is_none());

    // Blocked first, then by count.
    assert_eq!(entries[0].status, NetworkStatus::Blocked);
    let first_allowed = entries
        .iter()
        .position(|e| e.status == NetworkStatus::Allowed)
        .unwrap();
    assert!(
        entries[..first_allowed]
            .iter()
            .all(|e| e.status == NetworkStatus::Blocked)
    );
}
