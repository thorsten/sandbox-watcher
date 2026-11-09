# Sandbox network log fixtures

Hand-written samples of the three log files sandbox 0.73.2 keeps inside a
container, in the formats documented in its parsers
(`src/modules/network-diagnostics/*-parsing.ts`). The paths come from
`src/platform/container-system/diagnostics.ts`; the watcher reads all three
with one `cat` script per refresh (`network::logs_command`) instead of
starting `sandbox-container-tools network diagnostic <source>` three times,
which is a Node process each.

- `/var/log/firewall-blocked.log`: tcpdump on the NFLOG group, `-tt` epoch
  timestamps. Sandbox prints it as `network diagnostic firewall`.
- `/var/log/dns-proxy.log`: dnsmasq with `log-queries`. `config X is ...`
  lines are blocked domains, `reply X is <ip>` lines are resolutions.
  `network diagnostic dns`.
- `/var/log/proxy-access.log`: squid native access log. `TCP_DENIED` is
  blocked. `network diagnostic proxy-access`.

Replace with real captures from `sandbox network logs --raw` when available;
review for sensitive hosts first.
