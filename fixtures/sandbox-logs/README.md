# Sandbox network log fixtures

Hand-written samples of the three log files sandbox 0.73.1 keeps inside a
container, in the formats documented in its parsers
(`src/modules/network-diagnostics/*-parsing.ts`):

- `firewall-blocked.log`: tcpdump on the NFLOG group, `-tt` epoch timestamps.
  Read by `sandbox-container-tools network diagnostic firewall`.
- `dns-proxy.log`: dnsmasq with `log-queries`. `config X is ...` lines are
  blocked domains, `reply X is <ip>` lines are resolutions.
  Read by `... network diagnostic dns`.
- `proxy-access.log`: squid native access log. `TCP_DENIED` is blocked.
  Read by `... network diagnostic proxy-access`.

Replace with real captures from `sandbox network logs --raw` when available;
review for sensitive hosts first.
