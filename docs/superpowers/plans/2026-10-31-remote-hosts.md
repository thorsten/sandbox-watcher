# Remote Hosts over SSH Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Watch sandbox containers on remote machines reached over SSH next to local ones, with stop as the only remote action.

**Architecture:** One `ssh` master connection per configured host. Docker and Podman sockets on the remote are forwarded to private local Unix sockets and served by the existing `DockerAdapter`. Apple `container` runs its CLI through the master connection via a new `CommandRunner`. Every container carries a host and a container key (`<id>` local, `<alias>/<id>` remote) that the UI and Tauri commands use as identity.

**Tech Stack:** Rust 2024 (`sandbox-core`, Tauri 2 shell), tokio, bollard 0.21, system OpenSSH client, React + TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-31-remote-hosts-design.md`

**Refinement over the spec (agreed during planning):** instead of adding a `host` argument to every Tauri command, commands take a single container key `<alias>/<id>` (local: `<id>`), parsed on the Rust side. Aliases cannot contain `/`, so parsing is unambiguous. Step 1 of Task 3 updates the spec's Identity section accordingly.

## Global Constraints

- Platforms: remote hosts work on macOS and Linux only. On Windows the crate must still compile; remote hosts are ignored and the settings section says "Remote hosts are not supported on Windows".
- Aliases must match `^[A-Za-z0-9._@-]+$`, must not start with `-`, must not be `local`, at most 255 characters.
- Every `ssh` invocation passes `-o BatchMode=yes` and puts `--` before the alias. The app never accepts host keys.
- Control and forward sockets live in one private directory per process, mode `0700`, removed on quit.
- Remote command arguments are always passed through `shell_quote`; container execs for Docker/Podman go through the forwarded API, never a remote shell.
- Per-host refresh timeout: 10 s (`REMOTE_TIMEOUT`). Reconnect backoff: 5 s doubling to 300 s, reset on "Test connection" and on wake from sleep.
- Host-side commands refuse remote keys with exactly `not available for remote hosts`: `open_project`, `open_in_editor`, `open_terminal`, `reveal_mount`, `preview_allow`, `apply_allow`.
- Never read environment variable values from containers; probes stay unchanged.
- Keep `src/types.ts` in sync with every serialised Rust type change (kebab-case enums, flat `ContainerOverview`).
- Commits: Conventional Commits; run before each commit: `cargo fmt --all --check`, `cargo clippy -p sandbox-core --all-targets -- -D warnings`, `cargo test -p sandbox-core`, and for UI or `src-tauri` changes also `pnpm lint && pnpm format:check && pnpm test && pnpm build` and the `src-tauri` compile check from `AGENTS.md`.

## Review Focus

1. Unix socket path length: macOS limits socket paths to 104 bytes and `$TMPDIR` is long there. Control and forward paths must stay below 100 bytes (test in Task 5).
2. Same project slug on two hosts with different config hashes must not mark either container stale (`stale_ids` groups by host and slug; test in Task 3).
3. A host going unreachable must not fire "container died", "session ended" or "container stopped" notifications, and coming back must not fire "container started" for containers that were running all along (test in Task 7).
4. A remote path containing spaces or quotes must survive the remote shell unchanged (`shell_quote` round trip through `sh -c` in Task 5; Apple fake-runner test in Task 4).
5. Existing settings and history files written by 0.5.0 must load unchanged (serde defaults; tests in Tasks 3 and 7).

---

### Task 1: Host identity types

**Files:**

- Create: `crates/sandbox-core/src/host.rs`
- Modify: `crates/sandbox-core/src/lib.rs` (add `pub mod host;`)

**Interfaces:**

- Produces:
  - `pub enum Host { Local, Remote(String) }` with `is_local(&self) -> bool`, `as_str(&self) -> &str`; serialises as the string `"local"` or the alias.
  - `pub struct RuntimeId { pub host: Host, pub kind: RuntimeKind }` (`Hash`, `Ord`).
  - `pub fn validate_alias(alias: &str) -> Result<(), String>`
  - `pub fn container_key(host: &Host, id: &str) -> String`
  - `pub fn parse_container_key(key: &str) -> (Host, String)`

- [ ] **Step 1: Write the failing tests** at the bottom of the new `host.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_aliases() {
        for ok in ["devbox", "me@vm.example.com", "build-01", "a_b.c"] {
            assert!(validate_alias(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "-oProxyCommand=x", "local", "a b", "a/b", "a;b", "a$b", "ä",
            &"x".repeat(256),
        ] {
            assert!(validate_alias(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn container_keys_round_trip() {
        let remote = Host::Remote("devbox".into());
        assert_eq!(container_key(&Host::Local, "abc"), "abc");
        assert_eq!(container_key(&remote, "abc"), "devbox/abc");
        assert_eq!(parse_container_key("abc"), (Host::Local, "abc".into()));
        assert_eq!(parse_container_key("devbox/abc"), (remote, "abc".into()));
    }

    #[test]
    fn host_serialises_as_plain_string() {
        assert_eq!(serde_json::to_string(&Host::Local).unwrap(), "\"local\"");
        assert_eq!(
            serde_json::to_string(&Host::Remote("devbox".into())).unwrap(),
            "\"devbox\""
        );
        let back: Host = serde_json::from_str("\"devbox\"").unwrap();
        assert_eq!(back, Host::Remote("devbox".into()));
        let local: Host = serde_json::from_str("\"local\"").unwrap();
        assert_eq!(local, Host::Local);
    }

    #[test]
    fn local_sorts_first() {
        let mut hosts = vec![Host::Remote("b".into()), Host::Local, Host::Remote("a".into())];
        hosts.sort();
        assert_eq!(hosts[0], Host::Local);
        assert_eq!(hosts[1], Host::Remote("a".into()));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sandbox-core host::`
Expected: compile error, `validate_alias` / `Host` not found.

- [ ] **Step 3: Implement** above the tests in `host.rs`:

```rust
//! Which machine a runtime or container lives on, and the container key the
//! UI and Tauri commands use to address a container across machines.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::RuntimeKind;

/// `Local` sorts before every remote host.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Host {
    #[default]
    Local,
    /// An SSH alias, validated by [`validate_alias`].
    Remote(String),
}

impl Host {
    pub fn is_local(&self) -> bool {
        matches!(self, Host::Local)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Host::Local => "local",
            Host::Remote(alias) => alias,
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Host {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        Ok(if value == "local" {
            Host::Local
        } else {
            Host::Remote(value)
        })
    }
}

/// One runtime on one machine.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuntimeId {
    pub host: Host,
    pub kind: RuntimeKind,
}

/// Reject anything `ssh` could read as an option or the remote shell could
/// interpret. `/` is excluded so container keys stay unambiguous.
pub fn validate_alias(alias: &str) -> Result<(), String> {
    if alias.is_empty() || alias.len() > 255 {
        return Err("host alias must be 1 to 255 characters".into());
    }
    if alias.starts_with('-') {
        return Err("host alias must not start with '-'".into());
    }
    if alias == "local" {
        return Err("'local' is reserved for this machine".into());
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-'))
    {
        return Err("host alias may only contain letters, digits, '.', '_', '@' and '-'".into());
    }
    Ok(())
}

/// `<id>` for local containers, `<alias>/<id>` for remote ones.
pub fn container_key(host: &Host, id: &str) -> String {
    match host {
        Host::Local => id.to_string(),
        Host::Remote(alias) => format!("{alias}/{id}"),
    }
}

/// Inverse of [`container_key`]. Container ids and names never contain `/`.
pub fn parse_container_key(key: &str) -> (Host, String) {
    match key.split_once('/') {
        Some((alias, id)) => (Host::Remote(alias.to_string()), id.to_string()),
        None => (Host::Local, key.to_string()),
    }
}
```

`RuntimeKind` needs `PartialOrd, Ord, Hash`; add them to its derive in `crates/sandbox-core/src/model.rs` if missing. Add `pub mod host;` to `lib.rs` in alphabetical order (after `history`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p sandbox-core host::`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/src/host.rs crates/sandbox-core/src/lib.rs crates/sandbox-core/src/model.rs
git commit -m "feat(core): host identity and container keys"
```

---

### Task 2: Hosted adapters in the overview and live runtime

Local-only behaviour stays identical; every container now carries `host` and `key`.

**Files:**

- Modify: `crates/sandbox-core/src/overview.rs`
- Modify: `crates/sandbox-core/src/live.rs`
- Modify: `src-tauri/src/watcher.rs`, `src-tauri/src/lib.rs` (callers)

**Interfaces:**

- Consumes: `Host`, `RuntimeId`, `container_key`, `parse_container_key` (Task 1).
- Produces:
  - `pub struct HostedAdapter { pub host: Host, pub adapter: Arc<dyn RuntimeAdapter> }` (`Clone`) in `overview.rs`, with `HostedAdapter::local(adapter)`.
  - `ContainerOverview` gains `pub host: Host`, `pub key: String`, `#[serde(default)] pub last_seen_at: Option<DateTime<Utc>>` (set only for containers of unreachable hosts, Task 7).
  - `RuntimeFailure` and `ConnectedRuntime` gain `#[serde(default)] pub host: Host`.
  - `pub async fn load_many(adapters: &[HostedAdapter], source, candidates) -> Overview`; `load(adapter, ...)` unchanged signature (wraps local).
  - `LiveRuntime::adapters(&self) -> Vec<HostedAdapter>`; `LiveRuntime::adapter_for(&self, key: &str) -> Option<(Arc<dyn RuntimeAdapter>, String)>` returning the adapter and the raw container id; `LiveRuntime::with_adapter(self: &Arc<Self>, host: Host, adapter)`.
  - `Watcher::adapter_for(&self, key: &str) -> Result<(Arc<dyn RuntimeAdapter>, String), String>`.

- [ ] **Step 1: Write failing tests** in `overview.rs` tests module:

```rust
    #[tokio::test]
    async fn same_container_id_on_two_hosts_stays_apart() {
        let local = HostedAdapter::local(Arc::new(MockAdapter::demo()));
        let remote = HostedAdapter {
            host: Host::Remote("devbox".into()),
            adapter: Arc::new(MockAdapter::demo()),
        };
        let overview = load_many(&[local, remote], DataSource::Mock, vec![]).await;
        assert_eq!(overview.containers.len(), 8);
        let keys: std::collections::HashSet<_> =
            overview.containers.iter().map(|c| c.key.clone()).collect();
        assert_eq!(keys.len(), 8, "keys are unique across hosts");
        let first_remote = overview
            .containers
            .iter()
            .find(|c| !c.host.is_local())
            .unwrap();
        assert_eq!(first_remote.key, format!("devbox/{}", first_remote.container.id));
        // Local first within running containers.
        assert!(overview.containers[0].host.is_local());
    }

    #[tokio::test]
    async fn serialises_host_and_key() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        let json = serde_json::to_value(&overview).unwrap();
        let first = &json["containers"][0];
        assert_eq!(first["host"], "local");
        assert_eq!(first["key"], first["id"]);
        assert!(first["last_seen_at"].is_null());
        assert_eq!(json["runtime"]["runtimes"][0]["host"], "local");
    }
```

And in `live.rs` tests replace `adapters_are_listed_in_stable_order` body's `with_adapter(...)` calls with `with_adapter(Host::Local, ...)`, map `live.adapters().iter().map(|a| a.adapter.kind())`, and add:

```rust
    #[tokio::test]
    async fn adapter_for_resolves_container_keys() {
        let live = Arc::new(LiveRuntime::new());
        live.with_adapter(Host::Local, Arc::new(MockAdapter::demo()));
        let overview = live.load_connected(vec![]).await;
        let first = &overview.containers[0];
        let (_, id) = live.adapter_for(&first.key).expect("known key");
        assert_eq!(id, first.container.id);
        assert!(live.adapter_for("devbox/unknown").is_none());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sandbox-core overview:: live::`
Expected: compile errors (`HostedAdapter`, `key`, `load_connected` missing).

- [ ] **Step 3: Implement `overview.rs`**

```rust
/// An adapter plus the machine it talks to.
#[derive(Clone)]
pub struct HostedAdapter {
    pub host: Host,
    pub adapter: Arc<dyn RuntimeAdapter>,
}

impl HostedAdapter {
    pub fn local(adapter: Arc<dyn RuntimeAdapter>) -> Self {
        Self { host: Host::Local, adapter }
    }
}
```

- Add `pub host: Host` (with `#[serde(default)]`) to `RuntimeFailure` and `ConnectedRuntime`; add `host`, `key`, `last_seen_at` to `ContainerOverview` as listed under Interfaces (field order: after `container`, before `runtime`).
- `load_one(hosted: HostedAdapter)`: take `let HostedAdapter { host, adapter } = hosted;`, set `host: host.clone()` on the failure and on `ConnectedRuntime`, and build each container with `key: container_key(&host, &container.id)`, `host: host.clone()`, `last_seen_at: None`.
- `load_many(adapters: &[HostedAdapter], ...)`: `join_all(adapters.iter().cloned().map(load_one))`. The `Unavailable` reason becomes `format!("{}: {}", label(f), f.reason)` with `label` = `f.kind.to_string()` for local, `format!("{} on {}", f.kind, f.host)` for remote.
- Sort: running first, then `a.host.cmp(&b.host)`, then runtime, then name.
- `load(adapter, ...)` calls `load_many(&[HostedAdapter::local(adapter)], ...)`.

- [ ] **Step 4: Implement `live.rs`**

- `Connections.adapters: HashMap<RuntimeId, Arc<dyn RuntimeAdapter>>`, `event_tasks: HashMap<RuntimeId, JoinHandle<()>>`, `by_container: HashMap<String /* key */, (Arc<dyn RuntimeAdapter>, String /* raw id */)>`.
- `adapters()` returns `Vec<HostedAdapter>` sorted by `RuntimeId` (Local first, then kind).
- `adapter_for(key)`: exact lookup in `by_container`, else the existing prefix fallback restricted to the same host: `let (host, id) = parse_container_key(key);` then find an entry whose parsed host equals `host` and whose raw id starts with `id`.
- `with_adapter(host, adapter)`, `drop_adapter(&RuntimeId)`, `start_events(id: RuntimeId, adapter)` keyed by `RuntimeId`.
- Extract the tail of `overview()` (from `let adapters = self.adapters();` to the end) into `pub async fn load_connected(self: &Arc<Self>, candidates: Vec<DetectedRuntime>) -> Overview` so tests can drive it. In it, failures drop `RuntimeId { host: failure.host.clone(), kind: failure.kind }`, and `by_container` is filled with `c.key -> (adapter, c.container.id)` looked up by `RuntimeId { host: c.host.clone(), kind: c.runtime }`.
- Local connect in `overview()` calls `self.with_adapter(Host::Local, adapter)`; stale-adapter removal only considers `Host::Local` ids.

- [ ] **Step 5: Update callers in `src-tauri`**

- `watcher.rs`: `Watcher::adapters()` returns `Vec<HostedAdapter>` (mock: `vec![HostedAdapter::local(adapter.clone())]`). `adapter_for(key)` returns `Result<(Arc<dyn RuntimeAdapter>, String), String>`; in mock mode, `let (_, id) = parse_container_key(key); Ok((adapter.clone(), id))`.
- In `refresh()`, stats sets use `hosted.adapter` and filter `c.host == hosted.host && c.runtime == hosted.adapter.kind()`; history insights use `let Ok((adapter, id)) = self.adapter_for(&c.key)` and pass `&id`; network loop iterates `for hosted in self.adapters()` using `hosted.adapter`.
- `lib.rs`: every `let adapter = state.adapter_for(&container_id)?;` becomes `let (adapter, container_id) = state.adapter_for(&container_id)?;` so later uses of `container_id` get the raw id. Keep the Tauri argument name `container_id` for now (renamed in Task 3).

- [ ] **Step 6: Run checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings`
Then compile `src-tauri` as described in `AGENTS.md` (`cargo clippy -p sandbox-watcher --target aarch64-apple-darwin` with the stubbed C compiler on Linux, or `cargo clippy -p sandbox-watcher` on macOS).
Expected: all green; existing overview tests unchanged except `.with_adapter` call sites.

- [ ] **Step 7: Commit**

```bash
git add crates/sandbox-core/src/overview.rs crates/sandbox-core/src/live.rs src-tauri/src/watcher.rs src-tauri/src/lib.rs
git commit -m "refactor(core): carry host and container key through the overview"
```

---

### Task 3: Container keys downstream (stats, network, notifications, history, UI)

**Files:**

- Modify: `docs/superpowers/specs/2026-10-31-remote-hosts-design.md` (Identity section)
- Modify: `crates/sandbox-core/src/stats_hub.rs`, `crates/sandbox-core/src/notify.rs`, `crates/sandbox-core/src/history.rs`
- Modify: `src-tauri/src/watcher.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/tray.rs`
- Modify: `src/types.ts`, `src/api.ts`, `src/api.test.ts`, `src/format.ts`, `src/test/fixtures.ts`, components using container identity
- Test: Rust unit tests in the touched modules; `src/format.test.ts`, `src/api.test.ts`

**Interfaces:**

- Consumes: `ContainerOverview.key`, `ContainerOverview.host` (Task 2).
- Produces:
  - `StatsHub::sync(&self, sets: &[(Arc<dyn RuntimeAdapter>, Vec<(String, String)>)])` — pairs of `(key, raw id)`; `snapshot()` keyed by key.
  - `Notification.container_id` holds the container key; remote titles are prefixed `"<alias>: "`.
  - `stale_ids(&Overview) -> HashSet<String>` returns keys, grouping by `(host, project_slug)`.
  - History records store the key in `container_id` (local keys equal ids, so old files load unchanged).
  - Tauri commands take `container_key: String` (JS: `containerKey`).
  - `pub fn require_local(key: &str) -> Result<(), String>` in `src-tauri/src/lib.rs`.
  - TS: `SandboxContainer` gains `host: string; key: string; last_seen_at: string | null`; `ConnectedRuntime` and `RuntimeFailure` gain `host: string`.

- [ ] **Step 1: Update the spec's Identity section**

Replace the bullet "Every Tauri command that takes `container_id` also takes `host`. `adapter_for(host, id)`." with:

```markdown
- Every Tauri command addresses a container by its container key: `<id>` for local
  containers, `<alias>/<id>` for remote ones (aliases cannot contain `/`). The Rust side
  parses the key with `parse_container_key`; `adapter_for(key)` returns the adapter and the
  raw id. Stats, network reports, notifications and history are keyed the same way.
```

- [ ] **Step 2: Write failing Rust tests**

In `notify.rs` tests (use the module's existing helpers for building an `Overview`; add a `remote(c: ContainerOverview, alias: &str) -> ContainerOverview` helper that sets `host` and `key`):

```rust
    #[test]
    fn remote_notifications_carry_host_and_key() {
        let before = overview_with(vec![]);
        let started = remote(running("c1", "web-shop"), "devbox");
        let after = overview_with(vec![started]);
        let out = diff(&before, &after, &NotificationSettings::default());
        assert_eq!(out[0].container_id, "devbox/c1");
        assert!(out[0].title.starts_with("devbox: "), "{}", out[0].title);
    }

    #[test]
    fn same_slug_on_two_hosts_is_not_stale() {
        let mut a = running("c1", "web-shop");
        a.container.config_hash = Some("aaaaaaaaaaaa".into());
        let mut b = remote(running("c1", "web-shop"), "devbox");
        b.container.config_hash = Some("bbbbbbbbbbbb".into());
        assert!(stale_ids(&overview_with(vec![a, b])).is_empty());
    }
```

If the module has no `overview_with`/`running` helpers, add them in the test module building `Overview { source: DataSource::Mock, runtime: RuntimeStatus::NotDetected, containers, tested_sandbox_version: String::new() }` and a running `ContainerOverview` with `host: Host::Local`, `key: id.into()`, `last_seen_at: None`, `readiness: Readiness::Ready`, empty sessions.

In `history.rs` tests:

```rust
    #[test]
    fn history_from_0_5_loads_and_keys_stay_ids() {
        let old = r#"{"containers":[{"container_id":"abc","name":"sandbox-x","project_slug":"x","runtime":"docker","started_at":"2026-10-30T09:00:00Z","ended_at":null,"last_seen_at":"2026-10-30T10:00:00Z"}],"sessions":[]}"#;
        let history: History = serde_json::from_str(old).expect("0.5 history loads");
        assert_eq!(history.containers[0].container_id, "abc");
    }
```

Adjust the JSON to the exact field set of `ContainerRecord`/`History` in `history.rs` (copy a record from an existing test fixture if one exists).

In `stats_hub.rs` tests (or a new test module), with `MockAdapter::demo()`:

```rust
    #[tokio::test]
    async fn snapshot_is_keyed_by_container_key() {
        let hub = StatsHub::new();
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let id = "c-web-shop".to_string(); // use a running id from MockAdapter::demo()
        hub.sync(&[(adapter, vec![(format!("devbox/{id}"), id.clone())])]);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(hub.snapshot().contains_key(&format!("devbox/{id}")));
    }
```

Replace `"c-web-shop"` with a running container id from `MockAdapter::demo()` (read `mock.rs`).

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p sandbox-core notify:: history:: stats_hub::`
Expected: failures on title prefix, stale grouping, and compile error on `sync` signature.

- [ ] **Step 4: Implement Rust changes**

- `stats_hub.rs`: `sync(&self, sets: &[(Arc<dyn RuntimeAdapter>, Vec<(String, String)>)])`; `wanted` holds `(ptr, adapter, key, id)`; `tracked` is keyed by key; `collect(adapter, id, history)` gets the raw id. `sync_one(adapter, running_ids)` maps each id to `(id.clone(), id.clone())`.
- `notify.rs`: `by_id` maps `c.key.as_str()`; `stale_ids` groups by `(c.host.clone(), c.container.project_slug.as_str())` and inserts `c.key.clone()`. Add

```rust
fn titled(c: &ContainerOverview, title: String) -> String {
    match &c.host {
        Host::Local => title,
        Host::Remote(alias) => format!("{alias}: {title}"),
    }
}
```

and wrap every `title: format!(...)` as `title: titled(now, format!(...))` (use `was` where the container comes from `previous`).

- `history.rs` `observe`: use `c.key.clone()` wherever it used `c.container.id.clone()`. `record_usage`/`record_files` callers in `watcher.rs` pass `&c.key`.
- `watcher.rs`: stats sets become `(hosted.adapter.clone(), Vec<(c.key.clone(), c.container.id.clone())>)`; after `collect_all(&hosted.adapter, ...)`, rewrite `report.container_id = container_key(&hosted.host, &report.container_id)` for every report.
- `tray.rs`: wherever the tray emits `select-container` or looks up a container by id, use `c.key`.
- `lib.rs`: rename every Tauri command parameter `container_id: String` to `container_key: String`, then `let (adapter, container_id) = state.adapter_for(&container_key)?;`. Add

```rust
/// Host-side actions need the container's files on this machine.
pub fn require_local(key: &str) -> Result<(), String> {
    if sandbox_core::host::parse_container_key(key).0.is_local() {
        Ok(())
    } else {
        Err("not available for remote hosts".into())
    }
}
```

and call `require_local(&container_key)?;` as the first line of `reveal_mount`, `open_project`, `open_in_editor`, `open_terminal`, `preview_allow`, `apply_allow`. In `get_project_path`, pass `cfg!(windows) && host.is_local()` as the Windows flag so remote paths stay POSIX. Lookups of `c.container.id == container_id` inside commands (e.g. `get_activity`) compare `c.key == container_key`.

- Add a unit test in `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn host_side_actions_refuse_remote_keys() {
        assert!(super::require_local("abc").is_ok());
        assert_eq!(
            super::require_local("devbox/abc").unwrap_err(),
            "not available for remote hosts"
        );
    }
}
```

- [ ] **Step 5: Run Rust checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings`, then the `src-tauri` compile check.
Expected: green.

- [ ] **Step 6: Write failing UI tests**

In `src/api.test.ts`, update every command expectation that passed `{ containerId: ... }` to `{ containerKey: ... }`. Add to `src/format.test.ts`:

```ts
it("does not mark the same slug on two hosts as stale", () => {
  const a = container({ id: "c1", key: "c1", host: "local", config_hash: "aaaaaaaaaaaa" });
  const b = container({ id: "c1", key: "devbox/c1", host: "devbox", config_hash: "bbbbbbbbbbbb" });
  expect(staleHashes([a, b]).size).toBe(0);
});
```

Run: `pnpm test`
Expected: FAIL (argument names, `key` missing in fixtures, stale grouping).

- [ ] **Step 7: Implement UI changes**

- `src/types.ts`: add to `SandboxContainer`:

```ts
/** `local` or the SSH alias of the remote host. */
host: string;
/** Identity across hosts: `<id>` locally, `<alias>/<id>` remotely. Pass this to commands. */
key: string;
/** Set while the container's host is unreachable: when it was last seen. */
last_seen_at: string | null;
```

and `host: string` on `ConnectedRuntime` and `RuntimeFailure`.

- `src/api.ts`: every function taking `containerId: string` takes `containerKey: string` and invokes with `{ containerKey }` (plus its other args).
- `src/test/fixtures.ts` `container()`: default `host: "local"`, `key: partial.key ?? id`, `last_seen_at: null`.
- `src/format.ts` `staleHashes`: group by `` `${c.host}\u0000${c.project_slug}` `` and return a set of `c.key`.
- Components: every place that identifies a container (`selectedId`, `onSelect`, `key={...}` React keys for containers, `network.find((r) => r.container_id === ...)`, stats lookups, `stale.has(...)`, API calls) uses `c.key` instead of `c.id`. Find them with `grep -rn "\.id\b" src/App.tsx src/components/*.tsx`; leave non-container ids (sessions, turns) alone. `onSelectContainer` already receives the key from the tray.

- [ ] **Step 8: Run all checks**

Run: `pnpm lint && pnpm format:check && pnpm test && pnpm build`, plus Rust checks.
Expected: green.

- [ ] **Step 9: Commit**

```bash
git add -A docs/superpowers/specs crates/sandbox-core/src src-tauri/src src
git commit -m "refactor: address containers by container key"
```

---

### Task 4: CommandRunner for local and remote CLI calls

**Files:**

- Create: `crates/sandbox-core/src/runner.rs`
- Modify: `crates/sandbox-core/src/lib.rs`, `crates/sandbox-core/src/apple.rs`, `crates/sandbox-core/src/endpoints.rs`

**Interfaces:**

- Produces:
  - `pub struct CommandOutput { pub exit_code: i64, pub stdout: String, pub stderr: String }`
  - `#[derive(Clone)] pub enum CommandRunner { Local, Ssh(Arc<crate::ssh::SshSession>) }` plus `#[cfg(test)] Fake(Arc<FakeRunner>)`.
  - `CommandRunner::run(&self, binary: &str, args: &[String], timeout: Duration) -> Result<CommandOutput>`
  - `CommandRunner::is_local(&self) -> bool`
  - `#[cfg(test)] pub struct FakeRunner` with `FakeRunner::new(responses: Vec<(&str, CommandOutput)>)` (key = `"<binary> <args joined by space>"`) and `calls() -> Vec<String>`.
  - `AppleContainerAdapter::with_runner(runner: CommandRunner, binary: impl Into<String>) -> Self` (`new(binary)` = `with_runner(CommandRunner::Local, binary)`).
  - `endpoints::docker_host_from_context(runner: &CommandRunner) -> Option<String>`, `endpoints::podman_socket(runner: &CommandRunner) -> Option<String>`, `endpoints::resolve(candidate)` unchanged.
- Consumes: nothing new. Task 5 provides `SshSession::run(&self, remote_command: &str, timeout: Duration) -> Result<CommandOutput>`; until then the `Ssh` variant is added in Task 5 (this task adds `Local` and `Fake` only).

- [ ] **Step 1: Write failing tests** in `runner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_runner_captures_output_and_exit_code() {
        let out = CommandRunner::Local
            .run("sh", &["-c".into(), "echo hi; echo err >&2; exit 3".into()], Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out.exit_code, 3);
        assert_eq!(out.stdout, "hi\n");
        assert_eq!(out.stderr, "err\n");
    }

    #[tokio::test]
    async fn local_runner_times_out() {
        let err = CommandRunner::Local
            .run("sh", &["-c".into(), "sleep 5".into()], Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn missing_binary_is_unavailable() {
        let err = CommandRunner::Local
            .run("sandbox-watcher-no-such-binary", &[], Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::RuntimeUnavailable { .. }), "{err}");
    }
}
```

In `apple.rs` tests:

```rust
    #[tokio::test]
    async fn apple_adapter_runs_through_the_runner() {
        let fake = Arc::new(FakeRunner::new(vec![(
            "container --version",
            CommandOutput { exit_code: 0, stdout: "container CLI version 0.6.0\n".into(), stderr: String::new() },
        )]));
        let adapter = AppleContainerAdapter::with_runner(CommandRunner::Fake(fake.clone()), "container");
        let info = adapter.ping().await.unwrap();
        assert_eq!(info.version.as_deref(), Some("container CLI version 0.6.0"));
        assert_eq!(fake.calls(), vec!["container --version"]);
    }
```

In `endpoints.rs` tests:

```rust
    #[tokio::test]
    async fn docker_context_is_read_through_the_runner() {
        let fake = Arc::new(FakeRunner::new(vec![(
            "docker context inspect --format {{.Endpoints.docker.Host}}",
            CommandOutput { exit_code: 0, stdout: "unix:///var/run/docker.sock\n".into(), stderr: String::new() },
        )]));
        assert_eq!(
            docker_host_from_context(&CommandRunner::Fake(fake)).await,
            Some("unix:///var/run/docker.sock".into())
        );
    }

    #[tokio::test]
    async fn podman_socket_falls_back_to_podman_info() {
        let fake = Arc::new(FakeRunner::new(vec![(
            "podman info --format {{.Host.RemoteSocket.Path}}",
            CommandOutput { exit_code: 0, stdout: "/run/user/1000/podman/podman.sock\n".into(), stderr: String::new() },
        )]));
        assert_eq!(
            podman_socket(&CommandRunner::Fake(fake)).await,
            Some("/run/user/1000/podman/podman.sock".into())
        );
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sandbox-core runner:: apple:: endpoints::`
Expected: compile errors.

- [ ] **Step 3: Implement `runner.rs`**

```rust
//! Run a CLI on this machine or, through an SSH session, on a remote host.
//! Adapters and endpoint discovery go through this so the same code serves
//! both.

use std::process::Stdio;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::time::Duration;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone)]
pub enum CommandRunner {
    Local,
    #[cfg(test)]
    Fake(Arc<FakeRunner>),
}

impl CommandRunner {
    pub fn is_local(&self) -> bool {
        matches!(self, CommandRunner::Local)
    }

    pub async fn run(&self, binary: &str, args: &[String], timeout: Duration) -> Result<CommandOutput> {
        match self {
            CommandRunner::Local => run_local(binary, args, timeout).await,
            #[cfg(test)]
            CommandRunner::Fake(fake) => Ok(fake.answer(binary, args)),
        }
    }
}

async fn run_local(binary: &str, args: &[String], timeout: Duration) -> Result<CommandOutput> {
    let child = tokio::process::Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::RuntimeUnavailable {
            runtime: binary.to_string(),
            reason: format!("cannot start {binary}: {e}"),
        })?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| Error::Protocol(format!("{binary} {} timed out", args.join(" "))))??;
    Ok(CommandOutput {
        exit_code: output.status.code().map(i64::from).unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Canned answers keyed by `"<binary> <args>"`; unknown commands exit 127.
#[cfg(test)]
pub struct FakeRunner {
    responses: Vec<(String, CommandOutput)>,
    calls: Mutex<Vec<String>>,
}

#[cfg(test)]
impl FakeRunner {
    pub fn new(responses: Vec<(&str, CommandOutput)>) -> Self {
        Self {
            responses: responses.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            calls: Mutex::default(),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn answer(&self, binary: &str, args: &[String]) -> CommandOutput {
        let key = std::iter::once(binary.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        self.calls.lock().unwrap().push(key.clone());
        self.responses
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or(CommandOutput {
                exit_code: 127,
                stdout: String::new(),
                stderr: format!("{binary}: not found"),
            })
    }
}
```

Add `pub mod runner;` to `lib.rs`.

- [ ] **Step 4: Move `apple.rs` onto the runner**

- Remove the private `CommandOutput` struct and `use std::process::Stdio;`; import `crate::runner::{CommandOutput, CommandRunner}`.
- `AppleContainerAdapter { binary: String, runner: CommandRunner }`; `new(binary)` delegates to `with_runner(CommandRunner::Local, binary)`.
- `run(&self, args, timeout)` becomes:

```rust
    async fn run(&self, args: &[String], timeout: Duration) -> Result<CommandOutput> {
        self.runner
            .run(&self.binary, args, timeout)
            .await
            .map_err(|e| match e {
                Error::RuntimeUnavailable { reason, .. } => Error::RuntimeUnavailable {
                    runtime: RuntimeKind::AppleContainer.to_string(),
                    reason,
                },
                other => other,
            })
    }
```

- Any other `tokio::process::Command` use in `apple.rs` (search for it) goes through `self.runner.run` the same way.

- [ ] **Step 5: Move `endpoints.rs` onto the runner**

- `async fn run(runner: &CommandRunner, binary: &str, args: &[&str]) -> Option<String>`: call `runner.run(binary, &args_as_strings, CLI_TIMEOUT)`, return `None` unless `exit_code == 0`, else `Some(stdout.trim().to_string())` (keep the existing trimming behaviour of the old `run`).
- `docker_host_from_context(runner)` and `podman_socket(runner)` take the runner; `podman_socket` reads `CONTAINER_HOST` only when `runner.is_local()`.
- `resolve(candidate)` passes `&CommandRunner::Local`.

- [ ] **Step 6: Run checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings`
Expected: green, including the existing Apple fixture tests.

- [ ] **Step 7: Commit**

```bash
git add crates/sandbox-core/src/runner.rs crates/sandbox-core/src/lib.rs crates/sandbox-core/src/apple.rs crates/sandbox-core/src/endpoints.rs
git commit -m "refactor(core): run runtime CLIs through a CommandRunner"
```

---

### Task 5: SSH session

**Files:**

- Create: `crates/sandbox-core/src/ssh.rs`
- Modify: `crates/sandbox-core/src/lib.rs`, `crates/sandbox-core/src/runner.rs` (add `Ssh` variant)

**Interfaces:**

- Consumes: `validate_alias` (Task 1), `CommandOutput`, `CommandRunner` (Task 4).
- Produces:
  - `pub fn shell_quote(arg: &str) -> String`
  - `pub fn remote_command(binary: &str, args: &[String]) -> String`
  - `pub fn ssh_hint(stderr: &str, alias: &str) -> String` — maps ssh errors to the spec's user messages.
  - `pub struct SshSession` (Unix only, `#[cfg(unix)]`) with:
    - `pub async fn open(alias: &str, timeout: Duration) -> Result<SshSession>`
    - `pub fn alias(&self) -> &str`
    - `pub async fn check(&self) -> bool`
    - `pub async fn run(&self, remote_command: &str, timeout: Duration) -> Result<CommandOutput>`
    - `pub async fn forward(&self, remote_socket: &str) -> Result<PathBuf>`
    - `pub async fn close(&self)`
    - `impl Drop` doing a best-effort synchronous `ssh -O exit` and removing its directory.
  - `CommandRunner::Ssh(Arc<SshSession>)` (`#[cfg(unix)]`), whose `run(binary, args, timeout)` calls `session.run(&remote_command(binary, args), timeout)`.
  - Pure builders used by tests: `master_args(control: &Path, alias: &str) -> Vec<String>`, `control_args(control: &Path, alias: &str, op: &str) -> Vec<String>`, `run_args(control: &Path, alias: &str, remote: &str) -> Vec<String>`, `forward_args(control: &Path, alias: &str, local: &Path, remote: &str) -> Vec<String>`.

- [ ] **Step 1: Write failing tests** in `ssh.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn quotes_for_the_remote_shell() {
        assert_eq!(shell_quote("plain-arg_1.2/x:y@z"), "plain-arg_1.2/x:y@z");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("$HOME;rm -rf /|`x`\n"), "'$HOME;rm -rf /|`x`\n'");
    }

    /// `ssh` hands the command line to the remote login shell, which parses it
    /// once. `sh -c` here plays that shell.
    #[cfg(unix)]
    #[test]
    fn quoted_arguments_survive_the_remote_shell() {
        let args: Vec<String> = ["a b", "it's", "$HOME", "x;y", "`z`", "", "line\nbreak", "{}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut printf_args = vec!["%s\\0".to_string()];
        printf_args.extend(args.iter().cloned());
        let line = remote_command("printf", &printf_args);
        let out = std::process::Command::new("sh").arg("-c").arg(&line).output().unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        let got: Vec<String> = stdout
            .strip_suffix('\0')
            .unwrap()
            .split('\0')
            .map(str::to_string)
            .collect();
        assert_eq!(got, args);
    }

    #[test]
    fn builds_ssh_arguments_with_batch_mode_and_separator() {
        let control = Path::new("/tmp/sw-1-0/c");
        let master = master_args(control, "devbox");
        assert_eq!(master[..2], ["-M".to_string(), "-N".to_string()]);
        assert!(master.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(master.windows(2).any(|w| w == ["-o", "StreamLocalBindUnlink=yes"]));
        assert_eq!(master[master.len() - 2..], ["--".to_string(), "devbox".to_string()]);

        let run = run_args(control, "devbox", "docker version");
        assert_eq!(run[run.len() - 3..], ["--".to_string(), "devbox".into(), "docker version".into()]);

        let fwd = forward_args(control, "devbox", Path::new("/tmp/sw-1-0/f0.sock"), "/var/run/docker.sock");
        assert!(fwd.windows(2).any(|w| w == ["-O", "forward"]));
        assert!(fwd.windows(2).any(|w| w == ["-L", "/tmp/sw-1-0/f0.sock:/var/run/docker.sock"]));

        let exit = control_args(control, "devbox", "exit");
        assert!(exit.windows(2).any(|w| w == ["-O", "exit"]));
        assert_eq!(exit.last().unwrap(), "devbox");
    }

    #[cfg(unix)]
    #[test]
    fn socket_paths_fit_the_macos_limit() {
        let dir = private_dir().unwrap();
        let longest = dir.join("f99.sock");
        assert!(longest.as_os_str().len() < 100, "{}", longest.display());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn maps_ssh_errors_to_hints() {
        assert!(ssh_hint("Host key verification failed.", "devbox").contains("run `ssh devbox` once"));
        assert!(ssh_hint("devbox: Permission denied (publickey).", "devbox").contains("key or agent"));
        assert!(ssh_hint("ssh: Could not resolve hostname devbox", "devbox").contains("Could not resolve"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn open_rejects_invalid_aliases_without_running_ssh() {
        let err = SshSession::open("-oProxyCommand=touch /tmp/pwned", Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("must not start with '-'"), "{err}");
    }
}
```

`quoted_arguments_survive_the_remote_shell` and `socket_paths_fit_the_macos_limit` are Unix-only (`#[cfg(unix)]`), as is `open_rejects_invalid_aliases_without_running_ssh` (mark it `#[cfg(unix)]` too, since `SshSession` only exists there).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sandbox-core ssh::`
Expected: compile errors.

- [ ] **Step 3: Implement `ssh.rs`**

```rust
//! One SSH master connection per remote host, through the system `ssh`.
//!
//! Authentication, users, ports and jump hosts come from the user's
//! `~/.ssh/config` and agent. The app never prompts and never accepts host
//! keys (`BatchMode=yes`). Sockets live in a private `0700` directory.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::host::validate_alias;
use crate::runner::CommandOutput;
use crate::{Error, Result};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

/// Characters that never need quoting in a POSIX shell word.
fn is_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',')
}

/// Quote one argument for a POSIX shell.
pub fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(is_safe) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// The command line the remote login shell receives.
pub fn remote_command(binary: &str, args: &[String]) -> String {
    std::iter::once(shell_quote(binary))
        .chain(args.iter().map(|a| shell_quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What to tell the user about an ssh failure.
pub fn ssh_hint(stderr: &str, alias: &str) -> String {
    let text = stderr.trim();
    if text.contains("Host key verification failed") {
        format!("Add the host to known_hosts first: run `ssh {alias}` once in a terminal.")
    } else if text.contains("Permission denied") {
        "Key or agent authentication is required; the watcher cannot prompt for passwords.".into()
    } else if text.is_empty() {
        format!("ssh to {alias} failed without an error message.")
    } else {
        text.lines().last().unwrap_or(text).to_string()
    }
}

fn common_options() -> Vec<String> {
    [
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=10",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=2",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub fn master_args(control: &Path, alias: &str) -> Vec<String> {
    let mut args = vec!["-M".into(), "-N".into(), "-S".into(), control.display().to_string()];
    args.extend(common_options());
    args.extend(["-o".into(), "ControlPersist=no".into(), "-o".into(), "StreamLocalBindUnlink=yes".into()]);
    args.extend(["--".into(), alias.into()]);
    args
}

pub fn control_args(control: &Path, alias: &str, op: &str) -> Vec<String> {
    vec![
        "-S".into(), control.display().to_string(),
        "-o".into(), "BatchMode=yes".into(),
        "-O".into(), op.into(),
        "--".into(), alias.into(),
    ]
}

pub fn run_args(control: &Path, alias: &str, remote: &str) -> Vec<String> {
    let mut args = vec!["-S".into(), control.display().to_string()];
    args.extend(common_options());
    args.extend(["--".into(), alias.into(), remote.into()]);
    args
}

pub fn forward_args(control: &Path, alias: &str, local: &Path, remote: &str) -> Vec<String> {
    vec![
        "-S".into(), control.display().to_string(),
        "-o".into(), "BatchMode=yes".into(),
        "-O".into(), "forward".into(),
        "-L".into(), format!("{}:{remote}", local.display()),
        "--".into(), alias.into(),
    ]
}

/// A short, private directory: Unix socket paths are limited to 104 bytes on macOS.
fn private_dir() -> Result<PathBuf> {
    let base = if cfg!(target_os = "macos") { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let dir = base.join(format!("sw-{}-{}", std::process::id(), NEXT_DIR.fetch_add(1, Ordering::Relaxed)));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    Ok(dir)
}

async fn ssh(args: &[String], timeout: Duration) -> Result<CommandOutput> {
    crate::runner::CommandRunner::Local.run("ssh", args, timeout).await
}

pub struct SshSession {
    alias: String,
    dir: PathBuf,
    control: PathBuf,
    master: Mutex<Option<tokio::process::Child>>,
    forwards: AtomicUsize,
}

impl SshSession {
    pub async fn open(alias: &str, timeout: Duration) -> Result<SshSession> {
        validate_alias(alias).map_err(|reason| Error::RuntimeUnavailable { runtime: "ssh".into(), reason })?;
        let dir = private_dir()?;
        let control = dir.join("c");
        let mut child = tokio::process::Command::new("ssh")
            .args(master_args(&control, alias))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::RuntimeUnavailable { runtime: "ssh".into(), reason: format!("cannot start ssh: {e}") })?;
        let session = SshSession {
            alias: alias.to_string(),
            dir,
            control,
            master: Mutex::new(None),
            forwards: AtomicUsize::new(0),
        };
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = pipe.read_to_string(&mut stderr).await;
                }
                return Err(Error::RuntimeUnavailable {
                    runtime: format!("ssh {alias}"),
                    reason: format!("{} ({status})", ssh_hint(&stderr, alias)),
                });
            }
            if session.check().await {
                *session.master.lock().expect("ssh master poisoned") = Some(child);
                return Ok(session);
            }
            if Instant::now() >= deadline {
                let _ = child.start_kill();
                return Err(Error::RuntimeUnavailable {
                    runtime: format!("ssh {alias}"),
                    reason: format!("no connection within {}s", timeout.as_secs()),
                });
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Whether the master connection is still up.
    pub async fn check(&self) -> bool {
        ssh(&control_args(&self.control, &self.alias, "check"), Duration::from_secs(5))
            .await
            .map(|o| o.exit_code == 0)
            .unwrap_or(false)
    }

    pub async fn run(&self, remote_command: &str, timeout: Duration) -> Result<CommandOutput> {
        ssh(&run_args(&self.control, &self.alias, remote_command), timeout).await
    }

    /// Forward a remote Unix socket to a new local one and return its path.
    pub async fn forward(&self, remote_socket: &str) -> Result<PathBuf> {
        let n = self.forwards.fetch_add(1, Ordering::Relaxed);
        let local = self.dir.join(format!("f{n}.sock"));
        let out = ssh(&forward_args(&self.control, &self.alias, &local, remote_socket), Duration::from_secs(10)).await?;
        if out.exit_code != 0 {
            return Err(Error::RuntimeUnavailable {
                runtime: format!("ssh {}", self.alias),
                reason: format!("cannot forward {remote_socket}: {}", ssh_hint(&out.stderr, &self.alias)),
            });
        }
        Ok(local)
    }

    pub async fn close(&self) {
        let _ = ssh(&control_args(&self.control, &self.alias, "exit"), Duration::from_secs(5)).await;
        let child = self.master.lock().expect("ssh master poisoned").take();
        if let Some(mut child) = child {
            let _ = child.start_kill();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        let _ = std::process::Command::new("ssh")
            .args(control_args(&self.control, &self.alias, "exit"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
```

Gate the whole module with `#![cfg(unix)]` at the top of `ssh.rs`, except `shell_quote`, `remote_command` and `ssh_hint` which are platform-independent: put those three in a `pub mod quote` section above the `#[cfg(unix)]` items, or mark each Unix-only item with `#[cfg(unix)]`. Register `pub mod ssh;` in `lib.rs`. Run `cargo fmt` afterwards; the compact array formatting above will be reflowed.

- [ ] **Step 4: Add the `Ssh` runner variant** in `runner.rs`:

```rust
    #[cfg(unix)]
    Ssh(Arc<crate::ssh::SshSession>),
```

and in `run`:

```rust
            #[cfg(unix)]
            CommandRunner::Ssh(session) => {
                session
                    .run(&crate::ssh::remote_command(binary, args), timeout)
                    .await
            }
```

- [ ] **Step 5: Run checks**

Run: `cargo test -p sandbox-core ssh:: runner:: && cargo clippy -p sandbox-core --all-targets -- -D warnings`
Expected: green. Also check Windows compiles: `cargo check -p sandbox-core --target x86_64-pc-windows-gnu` if the target is installed; otherwise CI's Windows job covers it.

- [ ] **Step 6: Commit**

```bash
git add crates/sandbox-core/src/ssh.rs crates/sandbox-core/src/runner.rs crates/sandbox-core/src/lib.rs
git commit -m "feat(core): SSH master sessions with socket forwarding"
```

---

### Task 6: Remote detection and host connection state

**Files:**

- Create: `crates/sandbox-core/src/remote.rs`
- Modify: `crates/sandbox-core/src/lib.rs`, `crates/sandbox-core/src/endpoints.rs`

**Interfaces:**

- Consumes: `CommandRunner` (Tasks 4–5), `SshSession` (Task 5), `HostedAdapter` (Task 2), `config::parse_runtime`, `live::select_candidates`, `DockerAdapter::connect`, `AppleContainerAdapter::with_runner`.
- Produces:
  - `pub struct RemoteCandidate { pub kind: RuntimeKind, pub endpoint: String }`
  - `pub struct RemoteDetection { pub candidates: Vec<RemoteCandidate>, pub preferred: Option<RuntimeKind>, pub probes: Vec<String> }`
  - `pub async fn detect_remote(runner: &CommandRunner) -> RemoteDetection`
  - `pub async fn resolve_remote(runner: &CommandRunner, kind: RuntimeKind) -> Option<String>` in `endpoints.rs`.
  - `#[serde(tag = "state", rename_all = "kebab-case")] pub enum HostState { Disabled, Connecting, Connected, Unreachable { reason: String, since: DateTime<Utc> } }`
  - `pub struct HostStatus { pub alias: String, pub display_name: String, pub state: HostState, pub runtimes: Vec<RuntimeKind>, pub probes: Vec<String> }` (Serialize).
  - `pub struct Backoff` with `new()`, `next_delay(&mut self) -> Duration`, `reset(&mut self)`.
  - `#[cfg(unix)] pub async fn connect_host(alias: &str, timeout: Duration) -> std::result::Result<(Arc<SshSession>, Vec<HostedAdapter>, RemoteDetection), (String, Vec<String>)>` — error carries reason and probe lines.

- [ ] **Step 1: Write failing tests** in `remote.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{CommandOutput, FakeRunner};

    fn ok(stdout: &str) -> CommandOutput {
        CommandOutput { exit_code: 0, stdout: stdout.into(), stderr: String::new() }
    }

    const WHICH: &str = "sh -c command -v docker; command -v podman; command -v container; true";
    const CONFIG: &str =
        "sh -c cat \"${XDG_CONFIG_HOME:-$HOME/.config}/sandbox/config.toml\" 2>/dev/null; true";

    #[tokio::test]
    async fn detects_docker_and_podman_in_sandbox_order() {
        let fake = Arc::new(FakeRunner::new(vec![
            (WHICH, ok("/usr/bin/podman\n/usr/bin/docker\n")),
            (CONFIG, ok("")),
            ("docker context inspect --format {{.Endpoints.docker.Host}}", ok("unix:///var/run/docker.sock\n")),
            ("podman info --format {{.Host.RemoteSocket.Path}}", ok("/run/user/1000/podman/podman.sock\n")),
        ]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        let kinds: Vec<_> = d.candidates.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, vec![RuntimeKind::Docker, RuntimeKind::Podman]);
        assert_eq!(d.candidates[0].endpoint, "unix:///var/run/docker.sock");
        assert_eq!(d.candidates[1].endpoint, "/run/user/1000/podman/podman.sock");
        assert_eq!(d.preferred, None);
    }

    #[tokio::test]
    async fn remote_sandbox_config_pins_the_runtime() {
        let fake = Arc::new(FakeRunner::new(vec![
            (WHICH, ok("/usr/bin/docker\n/usr/bin/podman\n")),
            (CONFIG, ok("runtime = \"podman\"\n")),
            ("podman info --format {{.Host.RemoteSocket.Path}}", ok("/run/podman/podman.sock\n")),
        ]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        assert_eq!(d.preferred, Some(RuntimeKind::Podman));
        assert_eq!(d.candidates.len(), 1);
        assert_eq!(d.candidates[0].kind, RuntimeKind::Podman);
    }

    #[tokio::test]
    async fn docker_without_context_uses_the_default_socket() {
        let fake = Arc::new(FakeRunner::new(vec![(WHICH, ok("/usr/bin/docker\n")), (CONFIG, ok(""))]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        assert_eq!(d.candidates[0].endpoint, "unix:///var/run/docker.sock");
    }

    #[tokio::test]
    async fn nothing_found_reports_every_probe() {
        let fake = Arc::new(FakeRunner::new(vec![(WHICH, ok("")), (CONFIG, ok(""))]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        assert!(d.candidates.is_empty());
        assert!(d.probes.iter().any(|p| p.contains("docker: not found")), "{:?}", d.probes);
        assert!(d.probes.iter().any(|p| p.contains("podman: not found")));
        assert!(d.probes.iter().any(|p| p.contains("container: not found")));
    }

    #[test]
    fn backoff_doubles_to_five_minutes_and_resets() {
        let mut b = Backoff::new();
        let delays: Vec<u64> = (0..8).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(delays, vec![5, 10, 20, 40, 80, 160, 300, 300]);
        b.reset();
        assert_eq!(b.next_delay().as_secs(), 5);
    }

    #[test]
    fn host_state_serialises_for_the_ui() {
        let s = HostState::Unreachable { reason: "x".into(), since: chrono::Utc::now() };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["state"], "unreachable");
        assert_eq!(json["reason"], "x");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sandbox-core remote::`
Expected: compile errors.

- [ ] **Step 3: Implement `endpoints::resolve_remote`**

```rust
/// Endpoint of a runtime on a remote host, discovered through its CLI there.
/// Docker falls back to the default socket; TCP endpoints are passed through
/// and rejected by the caller.
pub async fn resolve_remote(runner: &CommandRunner, kind: RuntimeKind) -> Option<String> {
    match kind {
        RuntimeKind::Docker => Some(
            docker_host_from_context(runner)
                .await
                .unwrap_or_else(|| "unix:///var/run/docker.sock".into()),
        ),
        RuntimeKind::Podman => podman_socket(runner).await,
        RuntimeKind::AppleContainer => Some("container".into()),
    }
}
```

- [ ] **Step 4: Implement `remote.rs`**

```rust
//! Remote hosts: detect runtimes over SSH, build adapters for them, and
//! track each host's connection state.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::endpoints::resolve_remote;
use crate::model::RuntimeKind;
use crate::runner::CommandRunner;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const SANDBOX_ORDER: [(RuntimeKind, &str); 3] = [
    (RuntimeKind::Docker, "docker"),
    (RuntimeKind::Podman, "podman"),
    (RuntimeKind::AppleContainer, "container"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCandidate {
    pub kind: RuntimeKind,
    pub endpoint: String,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteDetection {
    pub candidates: Vec<RemoteCandidate>,
    pub preferred: Option<RuntimeKind>,
    pub probes: Vec<String>,
}

async fn sh(runner: &CommandRunner, script: &str) -> String {
    runner
        .run("sh", &["-c".into(), script.into()], PROBE_TIMEOUT)
        .await
        .map(|o| o.stdout)
        .unwrap_or_default()
}

pub async fn detect_remote(runner: &CommandRunner) -> RemoteDetection {
    let mut detection = RemoteDetection::default();
    let found = sh(runner, "command -v docker; command -v podman; command -v container; true").await;
    let present: Vec<&str> = found
        .lines()
        .filter_map(|l| l.trim().rsplit('/').next())
        .collect();
    let config = sh(
        runner,
        "cat \"${XDG_CONFIG_HOME:-$HOME/.config}/sandbox/config.toml\" 2>/dev/null; true",
    )
    .await;
    detection.preferred = crate::config::parse_runtime(&config);
    if let Some(kind) = detection.preferred {
        detection.probes.push(format!("sandbox config: runtime = {kind}"));
    }

    for (kind, binary) in SANDBOX_ORDER {
        if !present.contains(&binary) {
            detection.probes.push(format!("{binary}: not found on PATH"));
            continue;
        }
        if detection.preferred.is_some_and(|p| p != kind) {
            detection.probes.push(format!("{binary}: found, skipped (sandbox config pins another runtime)"));
            continue;
        }
        match resolve_remote(runner, kind).await {
            Some(endpoint) => {
                detection.probes.push(format!("{binary}: {endpoint}"));
                detection.candidates.push(RemoteCandidate { kind, endpoint });
            }
            None => detection.probes.push(format!("{binary}: found, but no endpoint")),
        }
    }
    detection
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum HostState {
    Disabled,
    Connecting,
    Connected,
    Unreachable { reason: String, since: DateTime<Utc> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostStatus {
    pub alias: String,
    pub display_name: String,
    #[serde(flatten)]
    pub state: HostState,
    pub runtimes: Vec<RuntimeKind>,
    pub probes: Vec<String>,
}

/// 5 s doubling to 5 min.
#[derive(Debug, Default)]
pub struct Backoff {
    attempt: u32,
}

impl Backoff {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_delay(&mut self) -> Duration {
        let secs = (5u64 << self.attempt.min(6)).min(300);
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_secs(secs)
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Open the master connection, detect runtimes and build adapters.
#[cfg(unix)]
pub async fn connect_host(
    alias: &str,
    timeout: Duration,
) -> std::result::Result<
    (Arc<crate::ssh::SshSession>, Vec<crate::overview::HostedAdapter>, RemoteDetection),
    (String, Vec<String>),
> {
    use crate::adapter::RuntimeAdapter;
    use crate::apple::AppleContainerAdapter;
    use crate::docker::DockerAdapter;
    use crate::host::Host;
    use crate::overview::HostedAdapter;

    let session = Arc::new(
        crate::ssh::SshSession::open(alias, timeout)
            .await
            .map_err(|e| (e.to_string(), vec![]))?,
    );
    let runner = CommandRunner::Ssh(session.clone());
    let mut detection = detect_remote(&runner).await;
    let host = Host::Remote(alias.to_string());
    let mut adapters = Vec::new();
    for candidate in &detection.candidates {
        let adapter: Option<Arc<dyn RuntimeAdapter>> = match candidate.kind {
            RuntimeKind::Docker | RuntimeKind::Podman => {
                let remote = candidate.endpoint.trim_start_matches("unix://");
                if !remote.starts_with('/') {
                    detection.probes.push(format!(
                        "{}: {} is not a Unix socket; only sockets are forwarded",
                        candidate.kind, candidate.endpoint
                    ));
                    None
                } else {
                    match session.forward(remote).await {
                        Ok(local) => DockerAdapter::connect(candidate.kind, &local.display().to_string())
                            .ok()
                            .map(|a| Arc::new(a) as Arc<dyn RuntimeAdapter>),
                        Err(e) => {
                            detection.probes.push(format!("{}: {e}", candidate.kind));
                            None
                        }
                    }
                }
            }
            RuntimeKind::AppleContainer => Some(Arc::new(AppleContainerAdapter::with_runner(
                runner.clone(),
                candidate.endpoint.clone(),
            ))),
        };
        if let Some(adapter) = adapter {
            adapters.push(HostedAdapter { host: host.clone(), adapter });
        }
    }
    if adapters.is_empty() {
        let probes = detection.probes.clone();
        session.close().await;
        return Err(("no usable container runtime found".into(), probes));
    }
    Ok((session, adapters, detection))
}
```

Register `pub mod remote;` in `lib.rs`. Note: with a preferred runtime, `detect_remote` filters before resolving, which is why `remote_sandbox_config_pins_the_runtime` only needs the podman answer.

- [ ] **Step 5: Run checks**

Run: `cargo test -p sandbox-core remote:: endpoints:: && cargo clippy -p sandbox-core --all-targets -- -D warnings`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add crates/sandbox-core/src/remote.rs crates/sandbox-core/src/endpoints.rs crates/sandbox-core/src/lib.rs
git commit -m "feat(core): detect runtimes on remote hosts"
```

---

### Task 7: Remote hosts in the live runtime and settings

**Files:**

- Modify: `crates/sandbox-core/src/settings.rs`, `crates/sandbox-core/src/live.rs`, `crates/sandbox-core/src/overview.rs`, `crates/sandbox-core/src/notify.rs`
- Modify: `src-tauri/src/watcher.rs`, `src-tauri/src/lib.rs`
- Modify: `src/types.ts`

**Interfaces:**

- Consumes: `connect_host`, `HostStatus`, `HostState`, `Backoff` (Task 6); `HostedAdapter`, `load_many` (Task 2).
- Produces:
  - `pub struct RemoteHost { pub alias: String, pub display_name: Option<String>, pub enabled: bool }` in `settings.rs`; `Settings.remote_hosts: Vec<RemoteHost>` (serde default empty); `normalised()` drops invalid aliases and duplicates.
  - `Overview.hosts: Vec<HostStatus>` (`#[serde(default)]`).
  - `LiveRuntime::set_remote_hosts(&self, hosts: Vec<RemoteHost>)`, `LiveRuntime::reset_backoff(&self)`, `LiveRuntime::shutdown(&self)`.
  - `pub const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);` in `live.rs`.
  - `pub async fn test_remote_host(alias: &str) -> HostStatus` in `live.rs` (opens a throwaway connection, detects, closes).
  - Tauri commands `test_remote_host(alias: String) -> HostStatus`.
  - `notify::diff` skips containers with `last_seen_at.is_some()` on either side.

- [ ] **Step 1: Write failing tests**

`settings.rs`:

```rust
    #[test]
    fn remote_hosts_default_empty_and_are_normalised() {
        let old: Settings = serde_json::from_str(r#"{"poll_interval_secs":10}"#).unwrap();
        assert!(old.remote_hosts.is_empty(), "0.5 settings load without remote hosts");

        let s = Settings {
            remote_hosts: vec![
                RemoteHost { alias: "devbox".into(), display_name: None, enabled: true },
                RemoteHost { alias: "devbox".into(), display_name: Some("dup".into()), enabled: true },
                RemoteHost { alias: "-oProxyCommand=x".into(), display_name: None, enabled: true },
            ],
            ..Settings::default()
        }
        .normalised();
        assert_eq!(s.remote_hosts.len(), 1);
        assert_eq!(s.remote_hosts[0].alias, "devbox");
    }
```

`notify.rs`:

```rust
    #[test]
    fn unreachable_hosts_do_not_notify() {
        let mut c = remote(running("c1", "web-shop"), "devbox");
        c.sessions = vec![/* one session built like the module's other tests */];
        let before = overview_with(vec![c.clone()]);
        let mut gone = c.clone();
        gone.last_seen_at = Some(chrono::Utc::now());
        gone.sessions.clear();
        let during = overview_with(vec![gone.clone()]);
        assert!(diff(&before, &during, &NotificationSettings::default()).is_empty());
        // Coming back with the same running container is not "started".
        assert!(diff(&during, &before, &NotificationSettings::default()).is_empty());
    }
```

Fill the session with the same constructor the module's existing session tests use.

`live.rs` (pure part, no SSH): extract the merge of unreachable snapshots into a function and test it:

```rust
    #[test]
    fn unreachable_host_keeps_last_snapshot_greyed() {
        let mut c = test_container("devbox/c1"); // ContainerOverview with host Remote("devbox")
        c.last_seen_at = None;
        let since = chrono::Utc::now();
        let kept = stale_snapshot(&[c], since);
        assert_eq!(kept[0].last_seen_at, Some(since));
        assert!(kept[0].sessions.is_empty(), "no live sessions claimed while unreachable");
    }
```

Add `fn test_container(key: &str) -> ContainerOverview` to the test module (host parsed from the key, running, `Readiness::Ready`, one session).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sandbox-core settings:: notify:: live::`
Expected: compile errors.

- [ ] **Step 3: Implement settings and notify**

`settings.rs`:

```rust
/// A machine reached through the user's own `ssh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHost {
    /// Alias from `~/.ssh/config`, or `user@host`.
    pub alias: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}
```

Add `pub remote_hosts: Vec<RemoteHost>` to `Settings` (default `vec![]`), and in `normalised()`:

```rust
        let mut seen = std::collections::HashSet::new();
        self.remote_hosts.retain(|h| {
            crate::host::validate_alias(&h.alias).is_ok() && seen.insert(h.alias.clone())
        });
```

`notify.rs` `diff`: at the start of the loop over `after`, `if now.last_seen_at.is_some() { continue; }`; inside `Some(was)`, `if was.last_seen_at.is_some() { continue; }` before comparing; in the loop over containers present only in `before`, skip those whose host has any container in `after` with `last_seen_at.is_some()` (compute `let unreachable_hosts: HashSet<&Host> = next.containers.iter().filter(|c| c.last_seen_at.is_some()).map(|c| &c.host).collect();` once).

- [ ] **Step 4: Implement the live runtime part**

In `overview.rs` add `#[serde(default)] pub hosts: Vec<HostStatus>` to `Overview` (initialised empty in `Overview::with`).

In `live.rs`:

```rust
pub const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

/// Containers of an unreachable host as last seen: greyed out, no live sessions.
pub fn stale_snapshot(containers: &[ContainerOverview], since: DateTime<Utc>) -> Vec<ContainerOverview> {
    containers
        .iter()
        .cloned()
        .map(|mut c| {
            c.last_seen_at = Some(c.last_seen_at.unwrap_or(since));
            c.sessions.clear();
            c
        })
        .collect()
}

struct RemoteConnection {
    config: RemoteHost,
    #[cfg(unix)]
    session: Option<Arc<crate::ssh::SshSession>>,
    adapters: Vec<HostedAdapter>,
    status: HostStatus,
    backoff: Backoff,
    retry_at: Option<std::time::Instant>,
    last_containers: Vec<ContainerOverview>,
}
```

`LiveRuntime` gains `remotes: tokio::sync::Mutex<Vec<RemoteConnection>>` and `last_refresh: Mutex<Option<std::time::Instant>>`.

- `set_remote_hosts(hosts)`: keep connections whose alias is still present and enabled (update `config`); close and drop the others (call `session.close()` in a spawned task); add new ones with state `Connecting` (or `Disabled` when `enabled == false`).
- `reset_backoff()`: for every connection, `backoff.reset(); retry_at = None`.
- In `overview()`, after the local part has collected its adapters:
  1. If `last_refresh` is older than `3 * poll interval` (pass the interval in via `set_poll_interval(Duration)` from the watcher; default 10 s), call `reset_backoff()` (treat as wake from sleep).
  2. For each enabled connection, in parallel (`join_all`):
     - If `session` is `Some` and `!session.check().await`: close it, set state `Unreachable { reason: "connection lost", since: now }`, clear adapters, `retry_at = now + backoff.next_delay()`.
     - If `session` is `None` and `retry_at` is `None` or past: `tokio::time::timeout(REMOTE_TIMEOUT, connect_host(alias, REMOTE_TIMEOUT))`. On success store session and adapters, state `Connected`, `runtimes` from adapters, `probes` from detection, `backoff.reset()`, and start event tasks with `self.with_adapter(host, adapter)`. On error, state `Unreachable { reason, since }`, `probes` from the error, `retry_at = now + backoff.next_delay()`.
  3. Build `all = local adapters + adapters of connected remotes` and call `load_many(&all, ...)`, where every remote adapter's `load_one` is wrapped in `tokio::time::timeout(REMOTE_TIMEOUT, ...)` (add a `timeout: Option<Duration>` argument to `load_one`, `Some(REMOTE_TIMEOUT)` when `!host.is_local()`; a timeout becomes a `RuntimeFailure { reason: "no answer within 10s" }`).
  4. A remote host whose adapters all appear in `failures` becomes `Unreachable` with the joined failure reasons, and its session is closed so the next round reconnects.
  5. For every connection: if `Connected`, store `last_containers` = its containers from the overview; if `Unreachable`, append `stale_snapshot(&last_containers, since)` to `overview.containers`.
  6. Set `overview.hosts` from every connection's `status`; record `last_refresh`.
- On Windows (`#[cfg(not(unix))]`), every configured host gets state `Unreachable { reason: "Remote hosts are not supported on Windows" }` and nothing is connected.
- `shutdown()`: close every session.
- `test_remote_host(alias)`: `connect_host(alias, REMOTE_TIMEOUT)` then close the session; map to a `HostStatus` with `Connected` and runtimes/probes, or `Unreachable` with reason/probes.

- [ ] **Step 5: Wire the Tauri shell**

- `watcher.rs`: next to both `live.set_preferred(settings.runtime_override);` calls add `live.set_remote_hosts(settings.remote_hosts.clone());` and `live.set_poll_interval(Duration::from_secs(settings.poll_interval_secs));`.
- `lib.rs`: add

```rust
#[tauri::command]
async fn test_remote_host(state: State<'_, App>, alias: String) -> Result<HostStatus, String> {
    sandbox_core::host::validate_alias(&alias)?;
    let status = sandbox_core::live::test_remote_host(&alias).await;
    state.reset_backoff();
    Ok(status)
}
```

(`App::reset_backoff` forwards to `LiveRuntime::reset_backoff` in live mode and does nothing in mock mode), register it in `generate_handler!`, and call `live.shutdown()` from the existing exit handling (`RunEvent::Exit` in the `.run(...)` closure; add the closure if the app uses plain `.run(context)`).

- `src/types.ts`: add

```ts
export interface RemoteHost {
  alias: string;
  display_name: string | null;
  enabled: boolean;
}

export type HostState =
  | { state: "disabled" }
  | { state: "connecting" }
  | { state: "connected" }
  | { state: "unreachable"; reason: string; since: string };

export type HostStatus = HostState & {
  alias: string;
  display_name: string;
  runtimes: RuntimeKind[];
  probes: string[];
};
```

`Settings.remote_hosts: RemoteHost[]`, `Overview.hosts: HostStatus[]`, and `src/api.ts` `testRemoteHost(alias: string): Promise<HostStatus>` invoking `test_remote_host` with `{ alias }` (add it to `api.test.ts`'s command table).

- [ ] **Step 6: Run checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings`, the `src-tauri` compile check, and `pnpm lint && pnpm format:check && pnpm test && pnpm build`.
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/sandbox-core/src src-tauri/src src/types.ts src/api.ts src/api.test.ts
git commit -m "feat: connect to remote hosts from settings"
```

---

### Task 8: UI for remote hosts and mock host

**Files:**

- Create: `src/hosts.ts`, `src/hosts.test.ts`, `src/components/RemoteHostsSection.tsx`, `src/components/__tests__/RemoteHosts.test.tsx`
- Modify: `src/components/Sidebar.tsx`, `src/components/ContainerActions.tsx`, `src/components/SettingsPanel.tsx`, `src/components/RuntimeBanner.tsx`, `src/components/FilesList.tsx`, `src/components/MountsPanel.tsx`, `src/components/NetworkPanel.tsx` (Allow button), `src/actions.ts`, `src/App.tsx`, `src/styles.css`, `src/test/fixtures.ts`
- Modify: `crates/sandbox-core/src/mock.rs`, `src-tauri/src/watcher.rs`

**Interfaces:**

- Consumes: `SandboxContainer.host/key/last_seen_at`, `Overview.hosts`, `HostStatus`, `RemoteHost`, `testRemoteHost` (Tasks 3, 7).
- Produces:
  - `src/hosts.ts`: `isRemote(c: SandboxContainer): boolean`, `hostLabel(host: string, hosts: HostStatus[]): string` (display name, else alias), `filterByHost(cs: SandboxContainer[], host: string | "all"): SandboxContainer[]`, `hostsInUse(cs: SandboxContainer[]): string[]` (local first, then aliases sorted).
  - `networkLogsCommand(projectDir: string | null, host?: string)`: prefixes `ssh <host> ` (host via `shellQuote`) for remote hosts.
  - `MockAdapter::remote_demo()` in `mock.rs`: one running container `sandbox-ml-pipeline` with a Claude Code session.
  - Mock mode runs `[HostedAdapter::local(demo), HostedAdapter { host: Remote("devbox"), adapter: remote_demo }]` and reports `hosts: [HostStatus { alias: "devbox", display_name: "devbox", state: Connected, runtimes: [Docker], probes: [] }]`.

- [ ] **Step 1: Write failing tests**

`src/hosts.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { filterByHost, hostLabel, hostsInUse, isRemote } from "./hosts";
import { container } from "./test/fixtures";
import type { HostStatus } from "./types";

const devbox: HostStatus = {
  alias: "devbox",
  display_name: "Dev VM",
  state: "connected",
  runtimes: ["docker"],
  probes: [],
};

describe("hosts", () => {
  const local = container({ id: "a" });
  const remote = container({ id: "b", key: "devbox/b", host: "devbox" });

  it("tells remote containers apart", () => {
    expect(isRemote(local)).toBe(false);
    expect(isRemote(remote)).toBe(true);
  });

  it("labels hosts with their display name", () => {
    expect(hostLabel("devbox", [devbox])).toBe("Dev VM");
    expect(hostLabel("other", [devbox])).toBe("other");
  });

  it("filters and lists hosts, local first", () => {
    expect(filterByHost([local, remote], "devbox")).toEqual([remote]);
    expect(filterByHost([local, remote], "all")).toHaveLength(2);
    expect(hostsInUse([remote, local])).toEqual(["local", "devbox"]);
  });
});
```

`src/actions.test.ts` addition:

```ts
it("prefixes the network logs command with ssh for remote hosts", () => {
  expect(networkLogsCommand("/home/me/app", "devbox")).toBe(
    "ssh devbox 'cd '\\''/home/me/app'\\'' && sandbox network logs --status=all'",
  );
  expect(networkLogsCommand(null, "devbox")).toBe("ssh devbox 'sandbox network logs --status=all'");
  expect(networkLogsCommand("/x", "local")).toBe("cd '/x' && sandbox network logs --status=all");
});
```

`src/components/__tests__/RemoteHosts.test.tsx` (static render via `src/test/render.tsx`, following the existing component tests):

```tsx
import { describe, expect, it } from "vitest";
import { render } from "../../test/render";
import { container } from "../../test/fixtures";
import { ContainerActions } from "../ContainerActions";
import { Sidebar } from "../Sidebar";
import { RemoteHostsSection } from "../RemoteHostsSection";

describe("remote hosts in the UI", () => {
  const remote = container({ id: "b", key: "devbox/b", host: "devbox" });

  it("hides host-side actions for remote containers but keeps stop", () => {
    const html = render(<ContainerActions container={remote} onChanged={() => {}} />);
    expect(html).not.toContain(">Terminal<");
    expect(html).not.toContain(">Folder<");
    expect(html).toContain("Stop");
    expect(html).toContain("remote host");
  });

  it("shows a host badge and last seen for unreachable hosts", () => {
    const greyed = { ...remote, last_seen_at: "2026-10-30T11:55:00Z" };
    const html = render(
      <Sidebar
        containers={[container({ id: "a" }), greyed]}
        runtime={{ status: "connected", runtimes: [], failures: [] }}
        hosts={[]}
        network={[]}
        selectedId={null}
        onSelect={() => {}}
        now={new Date("2026-10-30T12:00:00Z")}
      />,
    );
    expect(html).toContain("host-badge");
    expect(html).toContain("devbox");
    expect(html).toContain("last seen 5 min ago");
  });

  it("renders the settings section with an empty list", () => {
    const html = render(<RemoteHostsSection hosts={[]} statuses={[]} onChange={() => {}} />);
    expect(html).toContain("Remote hosts");
    expect(html).toContain("SSH alias");
    expect(html).toContain("Test connection");
  });
});
```

Adjust the `render` import, the `RuntimeStatus` literal and the "5 min ago" wording to the helpers already used in `src/components/__tests__/` and `src/format.ts` (`formatUptime` or the existing relative-time formatter).

Run: `pnpm test`
Expected: FAIL (modules and props missing).

- [ ] **Step 2: Implement `src/hosts.ts`**

```ts
import type { HostStatus, SandboxContainer } from "./types";

export function isRemote(c: SandboxContainer): boolean {
  return c.host !== "local";
}

export function hostLabel(host: string, hosts: HostStatus[]): string {
  return hosts.find((h) => h.alias === host)?.display_name || host;
}

export function filterByHost(cs: SandboxContainer[], host: string): SandboxContainer[] {
  return host === "all" ? cs : cs.filter((c) => c.host === host);
}

export function hostsInUse(cs: SandboxContainer[]): string[] {
  const remote = [...new Set(cs.map((c) => c.host).filter((h) => h !== "local"))].sort();
  return cs.some((c) => c.host === "local") ? ["local", ...remote] : remote;
}
```

- [ ] **Step 3: Implement `networkLogsCommand` host support** in `src/actions.ts`:

```ts
const shellQuote = (s: string) => `'${s.replaceAll("'", "'\\''")}'`;

export function networkLogsCommand(projectDir: string | null, host = "local"): string {
  const local = projectDir
    ? `cd ${shellQuote(projectDir)} && sandbox network logs --status=all`
    : "sandbox network logs --status=all";
  return host === "local" ? local : `ssh ${host} ${shellQuote(local)}`;
}
```

- [ ] **Step 4: Implement component changes**

- `ContainerActions.tsx`: `const remote = isRemote(container);`. When `remote`, render instead of the Terminal, Folder and Editor buttons a single `<span className="muted" title="Not available for remote hosts">remote host</span>`; keep Stop and Copy. `copy("network")` calls `networkLogsCommand(dir, container.host)`.
- `FilesList.tsx`, `MountsPanel.tsx`, `NetworkPanel.tsx`: hide "open in editor", "reveal" and "Allow" controls when `isRemote(container)` (each component already receives the container or its key; derive remoteness from `key.includes("/")` if only the key is available).
- `Sidebar.tsx`: new prop `hosts: HostStatus[]`. In `ContainerCard`, when `isRemote(c)`, render `<span className="host-badge">{hostLabel(c.host, hosts)}</span>` next to the runtime badge; when `c.last_seen_at`, add class `unreachable` to the card and render `last seen {relative(c.last_seen_at, now)}` using the existing relative-time formatter. Above the groups, when `hostsInUse(containers).length > 1`, render a `<select>` with "All hosts", "This Mac" / "This machine" for `local`, and each alias label; selection state lives in `App.tsx` and filtering uses `filterByHost`.
- `RuntimeBanner.tsx`: accept `hosts: HostStatus[]`; for each host with `state === "unreachable"`, render a warning line `"{label}: {reason}"` with a `<details>` listing `probes`.
- `RemoteHostsSection.tsx`:

```tsx
import { useState } from "react";
import { testRemoteHost } from "../api";
import type { HostStatus, RemoteHost } from "../types";

interface Props {
  hosts: RemoteHost[];
  statuses: HostStatus[];
  onChange: (hosts: RemoteHost[]) => void;
}

const ALIAS = /^[A-Za-z0-9._@-]+$/;

export function validAlias(alias: string): boolean {
  return ALIAS.test(alias) && !alias.startsWith("-") && alias !== "local" && alias.length <= 255;
}

export function RemoteHostsSection({ hosts, statuses, onChange }: Props) {
  const [draft, setDraft] = useState("");
  const [result, setResult] = useState<HostStatus | null>(null);
  const [testing, setTesting] = useState(false);
  const isWindows = navigator.userAgent.includes("Windows");

  const add = () => {
    const alias = draft.trim();
    if (!validAlias(alias) || hosts.some((h) => h.alias === alias)) return;
    onChange([...hosts, { alias, display_name: null, enabled: true }]);
    setDraft("");
  };

  const test = async (alias: string) => {
    setTesting(true);
    try {
      setResult(await testRemoteHost(alias));
    } finally {
      setTesting(false);
    }
  };

  return (
    <section className="settings-section">
      <h3>Remote hosts</h3>
      {isWindows ? (
        <p className="muted">Remote hosts are not supported on Windows.</p>
      ) : (
        <>
          <p className="muted">
            Uses your own <code>ssh</code> and <code>~/.ssh/config</code>. Key or agent
            authentication only; run <code>ssh &lt;alias&gt;</code> once first to accept the host
            key.
          </p>
          <ul className="remote-hosts">
            {hosts.map((h) => {
              const status = statuses.find((s) => s.alias === h.alias);
              return (
                <li key={h.alias}>
                  <label>
                    <input
                      type="checkbox"
                      checked={h.enabled}
                      onChange={(e) =>
                        onChange(
                          hosts.map((x) =>
                            x.alias === h.alias ? { ...x, enabled: e.target.checked } : x,
                          ),
                        )
                      }
                    />{" "}
                    {h.display_name || h.alias}
                  </label>
                  <span className="muted"> {status?.state ?? "not connected"}</span>
                  <button type="button" disabled={testing} onClick={() => void test(h.alias)}>
                    Test connection
                  </button>
                  <button
                    type="button"
                    onClick={() => onChange(hosts.filter((x) => x.alias !== h.alias))}
                  >
                    Remove
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="remote-host-add">
            <input
              placeholder="SSH alias, e.g. devbox"
              aria-label="SSH alias"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <button type="button" disabled={!validAlias(draft.trim())} onClick={add}>
              Add
            </button>
            <button
              type="button"
              disabled={testing || !validAlias(draft.trim())}
              onClick={() => void test(draft.trim())}
            >
              Test connection
            </button>
          </div>
          {result && (
            <div className={result.state === "connected" ? "notice ok" : "notice err"}>
              <strong>{result.alias}</strong>:{" "}
              {result.state === "unreachable"
                ? result.reason
                : `connected (${result.runtimes.join(", ")})`}
              <ul>
                {result.probes.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </section>
  );
}
```

Add a test `it("rejects option-like aliases", () => { expect(validAlias("-oProxyCommand=x")).toBe(false); expect(validAlias("devbox")).toBe(true); })` to the component test file.

- `SettingsPanel.tsx`: render `<RemoteHostsSection hosts={settings.remote_hosts} statuses={overview.hosts} onChange={(remote_hosts) => save({ ...settings, remote_hosts })} />` using the panel's existing save helper (pass `overview.hosts` down from `App.tsx` if the panel doesn't receive the overview yet).
- `App.tsx`: pass `hosts={overview.hosts}` to `Sidebar` and `RuntimeBanner`, hold the host filter state, apply `filterByHost` before rendering the sidebar.
- `src/styles.css`: `.host-badge` styled like the existing runtime badge with a distinct background; `.container-card.unreachable { opacity: 0.55; }`.
- `src/test/fixtures.ts`: `settings()` fixture gains `remote_hosts: []`; any `Overview` fixture gains `hosts: []`.

- [ ] **Step 5: Mock host**

- `mock.rs`: add `MockAdapter::remote_demo()` returning an adapter with one running container `sandbox-ml-pipeline` (project slug `ml-pipeline`, id `e5f6a7b8c9d0`) and a Claude Code session, built the same way `demo()` builds its containers. Add a test asserting it lists exactly one running container.
- `watcher.rs`: `Mode::Mock(Vec<HostedAdapter>)` holding the local demo and `HostedAdapter { host: Host::Remote("devbox".into()), adapter: Arc::new(MockAdapter::remote_demo()) }`; mock refresh uses `load_many` and then sets `overview.hosts = vec![HostStatus { alias: "devbox".into(), display_name: "devbox".into(), state: HostState::Connected, runtimes: vec![RuntimeKind::Docker], probes: vec![] }]`. Mock `adapter_for(key)` picks the adapter whose host matches `parse_container_key(key).0`.

- [ ] **Step 6: Run checks and look at it**

Run: `pnpm lint && pnpm format:check && pnpm test && pnpm build`, the Rust checks and the `src-tauri` compile check. Then on a machine with a display run `pnpm tauri:dev:mock` and confirm: the `devbox` badge on `sandbox-ml-pipeline`, no Terminal/Folder buttons on it, Stop present, the host filter, and the Remote hosts settings section.
Expected: green; UI as described.

- [ ] **Step 7: Commit**

```bash
git add src crates/sandbox-core/src/mock.rs src-tauri/src/watcher.rs
git commit -m "feat(ui): remote host badges, filter, settings and mock host"
```

---

### Task 9: SSH conformance in CI and documentation

**Files:**

- Modify: `crates/sandbox-core/tests/conformance.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`, `AGENTS.md`, `docs/TROUBLESHOOTING.md`, `docs/MILESTONES.md`

**Interfaces:**

- Consumes: `SshSession` (Task 5), `remote::connect_host` (Task 6), existing `conformance(adapter, slug)` in `tests/conformance.rs`.
- Produces: env switch `SANDBOX_WATCHER_SSH_HOST` that makes the real-runtime conformance test connect through SSH.

- [ ] **Step 1: Extend the conformance test**

In `real_runtime_adapter_conforms`, before building the adapter from `DOCKER_HOST`:

```rust
    #[cfg(unix)]
    if let Ok(alias) = std::env::var("SANDBOX_WATCHER_SSH_HOST") {
        let (session, adapters, _) = sandbox_core::remote::connect_host(&alias, std::time::Duration::from_secs(20))
            .await
            .expect("connect over ssh");
        let hosted = adapters
            .into_iter()
            .find(|a| a.adapter.kind().as_str() == runtime)
            .expect("runtime detected over ssh");
        conformance(hosted.adapter.clone(), &slug).await;

        // Reconnect after the master goes away.
        session.close().await;
        assert!(!session.check().await, "master is gone after close");
        let (again, adapters, _) = sandbox_core::remote::connect_host(&alias, std::time::Duration::from_secs(20))
            .await
            .expect("reconnect over ssh");
        assert!(!adapters.is_empty());
        again.close().await;
        return;
    }
```

`connect_host` returns `Vec<HostedAdapter>` (public type from `sandbox_core::overview`), so `.adapter` is available in integration tests.

- [ ] **Step 2: Add CI steps** to the `core` job in `.github/workflows/ci.yml`, after the Docker conformance step and before "Remove the test container" (the Docker test container must still be running):

```yaml
- name: Prepare SSH to localhost
  if: runner.os == 'Linux'
  run: |
    sudo apt-get update && sudo apt-get install -y openssh-server
    sudo systemctl start ssh
    mkdir -p ~/.ssh && chmod 700 ~/.ssh
    ssh-keygen -t ed25519 -N '' -f ~/.ssh/id_ed25519 -q
    cat ~/.ssh/id_ed25519.pub >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys
    ssh-keyscan -H localhost >> ~/.ssh/known_hosts 2>/dev/null
    ssh -o BatchMode=yes localhost docker version --format '{{.Server.Version}}'
- name: Docker conformance over SSH
  if: runner.os == 'Linux'
  env:
    SANDBOX_WATCHER_DOCKER_TESTS: "1"
    SANDBOX_WATCHER_TEST_SLUG: watcher-test
    SANDBOX_WATCHER_SSH_HOST: localhost
  run: cargo test -p sandbox-core --test conformance -- --nocapture
```

And after the Podman conformance step (Podman container still running), a "Podman conformance over SSH" step with `SANDBOX_WATCHER_RUNTIME: podman`, `SANDBOX_WATCHER_TEST_SLUG: watcher-podman`, `SANDBOX_WATCHER_SSH_HOST: localhost`. For Podman, remote detection reads `podman info` over SSH, which on the runner reports the rootless socket; the existing step already starts `podman system service` at `/tmp/podman.sock`, so also export `CONTAINER_HOST` for the remote side by adding `echo 'export CONTAINER_HOST=unix:///tmp/podman.sock' >> ~/.bashrc` is **not** enough (non-interactive shells skip it). Instead start the Podman service on its default rootless path in that step: replace `unix:///tmp/podman.sock` with `unix://$XDG_RUNTIME_DIR/podman/podman.sock` (create the directory first), and update the existing Podman step's `DOCKER_HOST` to the same path.

- [ ] **Step 3: Documentation**

- `AGENTS.md` rule "Never send data anywhere. The app only talks to the local runtime." becomes: "Never send data anywhere. The app only talks to the local runtime and to remote hosts the user added, through the user's own `ssh`." Add a rule: "Remote command arguments always go through `ssh::shell_quote`; host-side commands call `require_local` first."
- `README.md`: privacy section gets the same sentence; a new "Remote hosts" section: add an SSH alias in Settings, key or agent auth only, accept the host key with `ssh <alias>` once, supported runtimes (Docker, Podman, Apple `container`), what is unavailable remotely (terminal, folder, editor, reveal, allow list), Windows unsupported. Record the manual Apple `container` remote check like the local Apple note.
- `docs/TROUBLESHOOTING.md`: a "Remote hosts" section with the four messages from the spec's error table and their fixes, plus "Socket path too long" (should not happen; report a bug) and "Remote Docker socket denied" (add the remote user to the `docker` group or use rootless Docker).
- `docs/MILESTONES.md` backlog: change "Remote hosts over SSH or TCP Docker contexts." to "~~Remote hosts over SSH~~: shipped in 0.6.0 (watch and stop). TCP Docker contexts are not supported."

- [ ] **Step 4: Run all checks**

Run the full Rust and web check list from Global Constraints.
Expected: green locally (the SSH steps run in CI only). Push to a branch and confirm both SSH conformance steps pass in CI.

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/tests/conformance.rs .github/workflows/ci.yml README.md AGENTS.md docs/TROUBLESHOOTING.md docs/MILESTONES.md
git commit -m "test: run conformance over SSH in CI; document remote hosts"
```
