# Import ssh:// Docker contexts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** List the user's `ssh://` Docker contexts in Settings, Remote hosts, and add the selected ones as ordinary remote hosts.

**Architecture:** `sandbox-core/src/endpoints.rs` reads `docker context ls` through `CommandRunner` and turns each `ssh://` context into an `SshContext` (importable alias, or reason plus `~/.ssh/config` snippet). A read-only Tauri command returns the list (demo data in mock mode, empty on Windows). A presentational `DockerContextsList` component renders it inside `RemoteHostsSection`; adding a context appends a `RemoteHost` to the settings draft like the manual Add button does.

**Tech Stack:** Rust (tokio, serde, serde_json), Tauri 2, React + TypeScript, vitest with static rendering.

**Spec:** `docs/superpowers/specs/2026-10-31-docker-ssh-contexts-design.md`

## Global Constraints

- Listing contexts never opens an SSH connection; only hosts the user added are connected.
- No change to `RemoteHost`, `validate_alias`, `ssh.rs` argument builders, container keys or the live runtime.
- `docker` missing, non-zero exit or unparsable output: empty list, never an error.
- Import is a one-time copy: `RemoteHost { alias, display_name: Some(<context name>), enabled: true }`.
- Remote hosts are not supported on Windows: the command returns an empty list there.
- `src/types.ts` mirrors every serialised Rust type by hand; field names are snake_case as serde emits them.
- Persisted text (code comments, docs, commits) in plain English; Conventional Commits.

## Review Focus

- A context whose alias is already in the draft host list (added by hand or imported earlier): shown as "added", no checkbox, never added twice.
- The same alias produced by two contexts (`ssh://dev@box` under two names): adding both adds the alias once.
- Endpoint with a trailing `/` (`ssh://dev@box/`), as some Docker versions store it: importable, not "has a path".
- `docker context ls` printing a warning line before the JSON lines: the warning is skipped, the contexts still listed.
- Endpoint with an empty host (`ssh://`, `ssh://me@`): not importable, reason given, no snippet with an empty `HostName`.

Each line has a test in the task that owns the code (Task 1 for the last three, Task 3 for the first two).

## Spec deviation (recorded in Task 4)

The spec puts `added` on a Tauri row type. Settings edits a draft that is saved from the footer, so "added" must compare against the draft hosts, not the saved settings. The command therefore returns plain `SshContext` values and the UI computes "added" from its `hosts` prop. The list is loaded once when the section mounts. A snippet name that sanitises to an empty string becomes `docker-context`; `local` becomes `docker-local`.

---

### Task 1: Parse ssh:// Docker contexts in core

**Files:**

- Modify: `crates/sandbox-core/src/endpoints.rs` (new items above `#[cfg(test)]`, tests in the existing `mod tests`)
- Modify: `crates/sandbox-core/src/mock.rs` (add `demo_ssh_contexts`)

**Interfaces:**

- Consumes: `crate::host::validate_alias(&str) -> Result<(), String>`, `crate::runner::CommandRunner`, the private `run` helper in `endpoints.rs`.
- Produces:
  - `pub struct SshContext { pub name: String, pub endpoint: String, pub alias: Option<String>, pub reason: Option<String>, pub snippet: Option<String> }` (derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`)
  - `pub fn ssh_context(name: &str, endpoint: &str) -> SshContext`
  - `pub async fn docker_ssh_contexts(runner: &CommandRunner) -> Vec<SshContext>`
  - `pub fn mock::demo_ssh_contexts() -> Vec<SshContext>`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `endpoints.rs`:

```rust
    fn importable(alias: &str) -> (Option<String>, Option<String>, Option<String>) {
        (Some(alias.into()), None, None)
    }

    fn parts(c: &SshContext) -> (Option<String>, Option<String>, Option<String>) {
        (c.alias.clone(), c.reason.clone(), c.snippet.clone())
    }

    #[test]
    fn plain_ssh_endpoints_become_aliases() {
        assert_eq!(parts(&ssh_context("box", "ssh://dev@box")), importable("dev@box"));
        assert_eq!(parts(&ssh_context("box", "ssh://box")), importable("box"));
        assert_eq!(parts(&ssh_context("box", "ssh://dev@box/")), importable("dev@box"));
        assert_eq!(
            parts(&ssh_context("box", "ssh://dev@box.example.com")),
            importable("dev@box.example.com")
        );
    }

    #[test]
    fn ports_get_a_reason_and_a_snippet() {
        let c = ssh_context("vm", "ssh://me@vm:2222");
        assert_eq!(c.alias, None);
        assert_eq!(c.reason.as_deref(), Some("uses port 2222"));
        assert_eq!(
            c.snippet.as_deref(),
            Some("Host vm\n  HostName vm\n  User me\n  Port 2222\n")
        );
    }

    #[test]
    fn paths_get_a_reason_and_a_snippet_without_user() {
        let c = ssh_context("vm", "ssh://vm/run/docker.sock");
        assert_eq!(c.alias, None);
        assert_eq!(c.reason.as_deref(), Some("has a path"));
        assert_eq!(c.snippet.as_deref(), Some("Host vm\n  HostName vm\n"));
    }

    #[test]
    fn invalid_characters_and_ipv6_get_a_snippet() {
        let c = ssh_context("v6", "ssh://me@[::1]:22");
        assert_eq!(c.alias, None);
        assert_eq!(c.reason.as_deref(), Some("uses port 22"));
        assert_eq!(
            c.snippet.as_deref(),
            Some("Host v6\n  HostName ::1\n  User me\n  Port 22\n")
        );
        let c = ssh_context("odd", "ssh://me+x@box");
        assert_eq!(c.alias, None);
        assert_eq!(
            c.reason.as_deref(),
            Some("host alias may only contain letters, digits, '.', '_', '@' and '-'")
        );
        assert!(c.snippet.is_some());
    }

    #[test]
    fn empty_hosts_are_rejected_without_a_snippet() {
        for endpoint in ["ssh://", "ssh://me@", "ssh://me@:22"] {
            let c = ssh_context("x", endpoint);
            assert_eq!(c.alias, None, "{endpoint}");
            assert_eq!(c.reason.as_deref(), Some("has no host"), "{endpoint}");
            assert_eq!(c.snippet, None, "{endpoint}");
        }
    }

    #[test]
    fn invalid_ports_are_rejected() {
        let c = ssh_context("x", "ssh://me@box:99999");
        assert_eq!(c.reason.as_deref(), Some("has an invalid port"));
        assert_eq!(c.snippet, None);
    }

    #[test]
    fn snippet_names_are_sanitised() {
        let host_line = |name: &str| {
            ssh_context(name, "ssh://box:2222")
                .snippet
                .unwrap()
                .lines()
                .next()
                .unwrap()
                .to_string()
        };
        assert_eq!(host_line("my vm"), "Host my-vm");
        assert_eq!(host_line("-x"), "Host x");
        assert_eq!(host_line("local"), "Host docker-local");
        assert_eq!(host_line("äö"), "Host docker-context");
    }

    fn context_lines(stdout: &str) -> CommandRunner {
        CommandRunner::Fake(Arc::new(FakeRunner::new(vec![(
            "docker context ls --format {{json .}}",
            CommandOutput {
                exit_code: 0,
                stdout: stdout.into(),
                stderr: String::new(),
            },
        )])))
    }

    #[tokio::test]
    async fn lists_only_ssh_contexts() {
        let runner = context_lines(concat!(
            "WARNING: some docker warning\n",
            r#"{"Current":true,"Name":"default","DockerEndpoint":"unix:///var/run/docker.sock"}"#,
            "\n",
            r#"{"Current":false,"Name":"colima","DockerEndpoint":"unix:///Users/me/.colima/default/docker.sock"}"#,
            "\n",
            r#"{"Current":false,"Name":"remote","DockerEndpoint":"tcp://10.0.0.2:2376"}"#,
            "\n",
            r#"{"Current":false,"Name":"devbox","DockerEndpoint":"ssh://dev@devbox"}"#,
            "\n",
            r#"{"Current":false,"Name":"vm","DockerEndpoint":"ssh://me@vm:2222"}"#,
            "\n",
        ));
        let contexts = docker_ssh_contexts(&runner).await;
        let names: Vec<_> = contexts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["devbox", "vm"]);
        assert_eq!(contexts[0].alias.as_deref(), Some("dev@devbox"));
        assert_eq!(contexts[0].endpoint, "ssh://dev@devbox");
        assert_eq!(contexts[1].alias, None);
    }

    #[tokio::test]
    async fn missing_docker_or_garbage_gives_an_empty_list() {
        let missing = CommandRunner::Fake(Arc::new(FakeRunner::new(vec![])));
        assert!(docker_ssh_contexts(&missing).await.is_empty());
        assert!(docker_ssh_contexts(&context_lines("not json\n{\n")).await.is_empty());
    }

    #[test]
    fn demo_contexts_cover_both_row_kinds() {
        let demo = crate::mock::demo_ssh_contexts();
        assert!(demo.iter().any(|c| c.alias.is_some()));
        assert!(demo.iter().any(|c| c.snippet.is_some()));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sandbox-core endpoints::`
Expected: compile error, `cannot find function ssh_context` / `cannot find type SshContext`.

- [ ] **Step 3: Implement**

In `endpoints.rs`, add `use serde::{Deserialize, Serialize};` and `use crate::host::validate_alias;` to the imports, then add above `#[cfg(test)]`:

```rust
/// A Docker context with an `ssh://` endpoint, as offered for import into
/// the remote hosts. Listing contexts never connects to them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshContext {
    /// Docker context name.
    pub name: String,
    /// The `ssh://` endpoint as Docker reports it.
    pub endpoint: String,
    /// The alias to add, when the endpoint is a plain `user@host` or `host`.
    pub alias: Option<String>,
    /// Why the context cannot be added as is. Set exactly when `alias` is `None`.
    pub reason: Option<String>,
    /// A `~/.ssh/config` block that gives the context an alias. Set when
    /// `alias` is `None` and the endpoint names a usable host and port.
    pub snippet: Option<String>,
}

#[derive(Deserialize)]
struct ContextLine {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "DockerEndpoint", default)]
    docker_endpoint: String,
}

/// The `ssh://` contexts of the local Docker CLI. Empty when Docker is
/// missing or its output cannot be read.
pub async fn docker_ssh_contexts(runner: &CommandRunner) -> Vec<SshContext> {
    let Some(out) = run(runner, "docker", &["context", "ls", "--format", "{{json .}}"]).await
    else {
        return vec![];
    };
    out.lines()
        .filter_map(|line| serde_json::from_str::<ContextLine>(line.trim()).ok())
        .filter(|c| c.docker_endpoint.starts_with("ssh://"))
        .map(|c| ssh_context(&c.name, &c.docker_endpoint))
        .collect()
}

/// Turn one `ssh://[user@]host[:port][/path]` endpoint into an import row.
pub fn ssh_context(name: &str, endpoint: &str) -> SshContext {
    let rejected = |reason: &str, snippet: Option<String>| SshContext {
        name: name.into(),
        endpoint: endpoint.into(),
        alias: None,
        reason: Some(reason.into()),
        snippet,
    };
    let rest = endpoint.strip_prefix("ssh://").unwrap_or(endpoint);
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (user, host_port) = match authority.rsplit_once('@') {
        Some((user, host_port)) => (Some(user).filter(|u| !u.is_empty()), host_port),
        None => (None, authority),
    };
    let (host, port) = if let Some(v6) = host_port.strip_prefix('[') {
        match v6.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':')),
            None => (v6, None),
        }
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    if host.is_empty() {
        return rejected("has no host", None);
    }
    let port = match port {
        None => None,
        Some(p) => match p.parse::<u16>() {
            Ok(p) if p > 0 => Some(p),
            _ => return rejected("has an invalid port", None),
        },
    };
    let snippet = || Some(ssh_config_snippet(name, user, host, port));
    if let Some(port) = port {
        return rejected(&format!("uses port {port}"), snippet());
    }
    if !path.is_empty() {
        return rejected("has a path", snippet());
    }
    let alias = match user {
        Some(user) => format!("{user}@{host}"),
        None => host.to_string(),
    };
    match validate_alias(&alias) {
        Ok(()) => SshContext {
            name: name.into(),
            endpoint: endpoint.into(),
            alias: Some(alias),
            reason: None,
            snippet: None,
        },
        Err(reason) => rejected(&reason, snippet()),
    }
}

/// A `~/.ssh/config` block giving the endpoint an alias named after the context.
fn ssh_config_snippet(name: &str, user: Option<&str>, host: &str, port: Option<u16>) -> String {
    let mut out = format!("Host {}\n  HostName {host}\n", snippet_alias(name));
    if let Some(user) = user {
        out.push_str(&format!("  User {user}\n"));
    }
    if let Some(port) = port {
        out.push_str(&format!("  Port {port}\n"));
    }
    out
}

/// The context name reduced to characters an alias may contain.
fn snippet_alias(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    match mapped.trim_start_matches('-') {
        "" => "docker-context".into(),
        "local" => "docker-local".into(),
        other => other.into(),
    }
}
```

Bracketed IPv6 hosts without a port fail `validate_alias` on `:` and get its message as the reason. `snippet_alias("äö")` maps both characters to `-`, which trims to an empty string and becomes `docker-context`.

In `mock.rs`, add:

```rust
/// Docker contexts for demo mode: one importable, one that needs an alias.
pub fn demo_ssh_contexts() -> Vec<crate::endpoints::SshContext> {
    vec![
        crate::endpoints::ssh_context("staging", "ssh://dev@staging"),
        crate::endpoints::ssh_context("build-vm", "ssh://me@build-vm:2222"),
    ]
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sandbox-core endpoints::`
Expected: all `endpoints::tests` pass, including the existing three.

Then: `cargo clippy -p sandbox-core --all-targets -- -D warnings` and `cargo fmt --check`.
Expected: no warnings, no diff.

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/src/endpoints.rs crates/sandbox-core/src/mock.rs
git commit -m "feat(core): list ssh docker contexts for import"
```

---

### Task 2: Tauri command and TypeScript API

**Files:**

- Modify: `src-tauri/src/lib.rs` (new command next to `test_remote_host`, register in `generate_handler!`)
- Modify: `src/types.ts` (after `RemoteHost`)
- Modify: `src/api.ts` (after `testRemoteHost`)
- Test: `src/api.test.ts` (add a row to the invoke table)

**Interfaces:**

- Consumes: `sandbox_core::endpoints::{docker_ssh_contexts, SshContext}`, `sandbox_core::mock::demo_ssh_contexts`, `sandbox_core::runner::CommandRunner::Local`, `watcher::Mode`.
- Produces:
  - Tauri command `list_docker_ssh_contexts` returning `Vec<SshContext>`.
  - TS `export interface SshContext { name: string; endpoint: string; alias: string | null; reason: string | null; snippet: string | null; }`
  - TS `export function listDockerSshContexts(): Promise<SshContext[]>`

- [ ] **Step 1: Write the failing test**

In `src/api.test.ts`, add after the `"test remote host"` row:

```ts
    [
      "list docker ssh contexts",
      () => api.listDockerSshContexts(),
      "list_docker_ssh_contexts",
      undefined,
    ],
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm test src/api.test.ts`
Expected: FAIL, `api.listDockerSshContexts is not a function`.

- [ ] **Step 3: Implement**

`src/types.ts`, after `RemoteHost`:

```ts
/** A Docker context with an `ssh://` endpoint, offered for import. */
export interface SshContext {
  name: string;
  endpoint: string;
  /** Alias to add; null when the context needs an `~/.ssh/config` entry first. */
  alias: string | null;
  reason: string | null;
  snippet: string | null;
}
```

`src/api.ts`: add `SshContext` to the type import list, then after `testRemoteHost`:

```ts
/** The `ssh://` Docker contexts on this machine. Never connects to them. */
export function listDockerSshContexts(): Promise<SshContext[]> {
  return invoke<SshContext[]>("list_docker_ssh_contexts");
}
```

`src-tauri/src/lib.rs`, after `test_remote_host`:

```rust
/// The `ssh://` Docker contexts that can be imported as remote hosts.
/// Reads only; nothing is connected or saved.
#[tauri::command]
async fn list_docker_ssh_contexts(state: State<'_, App>) -> Result<Vec<SshContext>, String> {
    if matches!(state.mode, Mode::Mock(_)) {
        return Ok(sandbox_core::mock::demo_ssh_contexts());
    }
    if cfg!(not(unix)) {
        return Ok(vec![]);
    }
    Ok(sandbox_core::endpoints::docker_ssh_contexts(&CommandRunner::Local).await)
}
```

Add `use sandbox_core::endpoints::SshContext;` and `use sandbox_core::runner::CommandRunner;` to the imports (skip any already present), and `list_docker_ssh_contexts,` after `test_remote_host,` in `generate_handler!`.

- [ ] **Step 4: Verify**

Run: `pnpm test src/api.test.ts` — Expected: PASS.
Run: `cargo clippy -p sandbox-watcher --all-targets -- -D warnings` — Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src/types.ts src/api.ts src/api.test.ts
git commit -m "feat: command to list ssh docker contexts"
```

---

### Task 3: Docker contexts list in Settings

**Files:**

- Create: `src/components/DockerContextsList.tsx`
- Modify: `src/components/RemoteHostsSection.tsx`
- Modify: `src/styles.css` (only if `.docker-contexts` needs spacing; reuse `.remote-hosts` list styles otherwise)
- Test: `src/components/__tests__/RemoteHosts.test.tsx`

**Interfaces:**

- Consumes: `SshContext`, `RemoteHost` from `src/types.ts`; `listDockerSshContexts` from `src/api.ts`; `copyText` from `src/actions.ts`.
- Produces:
  - `export function contextsToAdd(contexts: SshContext[], selected: string[], hosts: RemoteHost[]): RemoteHost[]` — new hosts for the selected context names, skipping aliases already in `hosts` and duplicates within the selection.
  - `export function DockerContextsList(props: { contexts: SshContext[]; hosts: RemoteHost[]; onAdd: (added: RemoteHost[]) => void }): JSX.Element | null`

- [ ] **Step 1: Write the failing tests**

Add to `RemoteHosts.test.tsx` (extend the imports with `DockerContextsList, contextsToAdd` from `../DockerContextsList` and `SshContext, RemoteHost` from `../../types`):

```tsx
describe("docker ssh contexts", () => {
  const ctx = (name: string, endpoint: string, alias: string | null): SshContext => ({
    name,
    endpoint,
    alias,
    reason: alias ? null : "uses port 2222",
    snippet: alias ? null : `Host ${name}\n  HostName ${name}\n  Port 2222\n`,
  });
  const staging = ctx("staging", "ssh://dev@staging", "dev@staging");
  const vm = ctx("vm", "ssh://me@vm:2222", null);
  const host = (alias: string): RemoteHost => ({ alias, display_name: null, enabled: true });

  it("renders importable, added and unsupported rows", () => {
    const added = ctx("box", "ssh://box", "box");
    const { html, text } = render(
      <DockerContextsList contexts={[staging, added, vm]} hosts={[host("box")]} onAdd={noop} />,
    );
    expect(text).toContain("From Docker contexts");
    expect(text).toContain("staging");
    expect(text).toContain("ssh://dev@staging");
    expect(text).toContain("added");
    expect(text).toContain("uses port 2222");
    expect(text).toContain("Copy ~/.ssh/config entry");
    expect(count(html, 'type="checkbox"')).toBe(1);
    expect(text).toContain("Add selected");
  });

  it("renders nothing without contexts", () => {
    const { html } = render(<DockerContextsList contexts={[]} hosts={[]} onAdd={noop} />);
    expect(html).toBe("");
  });

  it("adds selected contexts once, named after the context", () => {
    const twin = ctx("staging-2", "ssh://dev@staging", "dev@staging");
    expect(contextsToAdd([staging, twin, vm], ["staging", "staging-2", "vm"], [])).toEqual([
      { alias: "dev@staging", display_name: "staging", enabled: true },
    ]);
  });

  it("skips aliases already in the host list", () => {
    expect(contextsToAdd([staging], ["staging"], [host("dev@staging")])).toEqual([]);
  });
});
```

Make sure `count` is imported from `../../test/render` alongside `render`.

- [ ] **Step 2: Run them to verify they fail**

Run: `pnpm test src/components/__tests__/RemoteHosts.test.tsx`
Expected: FAIL, cannot resolve `../DockerContextsList`.

- [ ] **Step 3: Implement**

`src/components/DockerContextsList.tsx`:

```tsx
import { useState } from "react";
import { copyText } from "../actions";
import type { RemoteHost, SshContext } from "../types";

interface Props {
  contexts: SshContext[];
  hosts: RemoteHost[];
  onAdd: (added: RemoteHost[]) => void;
}

/** New hosts for the selected contexts, skipping aliases already present. */
export function contextsToAdd(
  contexts: SshContext[],
  selected: string[],
  hosts: RemoteHost[],
): RemoteHost[] {
  const taken = new Set(hosts.map((h) => h.alias));
  const out: RemoteHost[] = [];
  for (const c of contexts) {
    if (!c.alias || !selected.includes(c.name) || taken.has(c.alias)) continue;
    taken.add(c.alias);
    out.push({ alias: c.alias, display_name: c.name, enabled: true });
  }
  return out;
}

export function DockerContextsList({ contexts, hosts, onAdd }: Props) {
  const [selected, setSelected] = useState<string[]>([]);
  if (contexts.length === 0) return null;
  const aliases = new Set(hosts.map((h) => h.alias));
  const toggle = (name: string, on: boolean) =>
    setSelected((s) => (on ? [...s, name] : s.filter((n) => n !== name)));
  const add = () => {
    onAdd(contextsToAdd(contexts, selected, hosts));
    setSelected([]);
  };

  return (
    <div className="docker-contexts">
      <h4>From Docker contexts</h4>
      <ul className="remote-hosts">
        {contexts.map((c) => (
          <li key={c.name}>
            {c.alias && aliases.has(c.alias) ? (
              <span>
                {c.name} <span className="muted">{c.endpoint} · added</span>
              </span>
            ) : c.alias ? (
              <label className="check">
                <input
                  type="checkbox"
                  checked={selected.includes(c.name)}
                  onChange={(e) => toggle(c.name, e.target.checked)}
                />{" "}
                {c.name} <span className="muted">{c.endpoint}</span>
              </label>
            ) : (
              <span className="muted">
                {c.name} {c.endpoint} · {c.reason}{" "}
                {c.snippet && (
                  <button type="button" onClick={() => void copyText(c.snippet ?? "")}>
                    Copy ~/.ssh/config entry
                  </button>
                )}
              </span>
            )}
          </li>
        ))}
      </ul>
      <button type="button" disabled={selected.length === 0} onClick={add}>
        Add selected
      </button>
    </div>
  );
}
```

`src/components/RemoteHostsSection.tsx`:

- Change the imports to `import { useEffect, useState } from "react";`, `import { listDockerSshContexts, testRemoteHost } from "../api";`, add `SshContext` to the type import, and `import { DockerContextsList } from "./DockerContextsList";`.
- Inside the component, after the existing `useState` calls:

```tsx
const [contexts, setContexts] = useState<SshContext[]>([]);

useEffect(() => {
  if (isWindows) return;
  listDockerSshContexts()
    .then(setContexts)
    .catch(() => setContexts([]));
}, [isWindows]);
```

- Render the list between the closing `</div>` of `remote-host-add` and `{testError && ...}`:

```tsx
<DockerContextsList
  contexts={contexts}
  hosts={hosts}
  onAdd={(added) => onChange([...hosts, ...added])}
/>
```

- [ ] **Step 4: Verify**

Run: `pnpm test` — Expected: all suites pass, including the existing `RemoteHosts` and `SettingsPanel` tests.
Run: `pnpm lint && pnpm exec tsc --noEmit` — Expected: clean.
Run: `pnpm tauri:dev:mock`, open Settings, Remote hosts: "staging" has a checkbox, "build-vm" shows "uses port 2222" and a copy button; ticking "staging" and clicking "Add selected" adds it to the list above and turns its row into "added".

- [ ] **Step 5: Commit**

```bash
git add src/components/DockerContextsList.tsx src/components/RemoteHostsSection.tsx src/components/__tests__/RemoteHosts.test.tsx src/styles.css
git commit -m "feat(ui): import ssh docker contexts as remote hosts"
```

---

### Task 4: Docs

**Files:**

- Modify: `README.md` (section "Remote hosts", around line 129)
- Modify: `docs/TROUBLESHOOTING.md` (line 65)
- Modify: `docs/superpowers/specs/2026-10-31-docker-ssh-contexts-design.md` (Tauri section and snippet naming)

- [ ] **Step 1: README**

After the bullet "Add an SSH alias from `~/.ssh/config` under Settings, then Remote hosts.", add:

```markdown
- Or pick hosts from your Docker contexts: Settings, Remote hosts, lists every
  `ssh://` context. Contexts with a port or path need an alias in
  `~/.ssh/config` first; the list offers the entry to copy.
```

- [ ] **Step 2: Troubleshooting**

Replace "Contexts that use `ssh://` are not supported." with:

```markdown
Contexts that use `ssh://` are not watched directly. Import them under
Settings, Remote hosts, From Docker contexts.
```

- [ ] **Step 3: Spec**

In the spec's "Tauri" section, replace the `DockerSshContextRow` bullet with: "`list_docker_ssh_contexts() -> Vec<SshContext>`. Read-only; uses `CommandRunner::Local`; demo data in mock mode. The UI marks a context as added when its alias is in the Settings draft, because Settings saves from its footer." Replace "and after hosts are saved" in the UI section with "once". In the snippet rule, replace "`docker-<name>` if the result is empty or `local`" with "`docker-context` if the result is empty, `docker-local` if it is `local`". In the UI section, replace "`src/types.ts` mirrors `DockerSshContextRow`" with "`src/types.ts` mirrors `SshContext`" and the mock contexts with `ssh://dev@staging` and `ssh://me@build-vm:2222`.

- [ ] **Step 4: Verify and commit**

Run: `pnpm exec oxfmt --check README.md docs` (or the repo's format hook via `git commit`).

```bash
git add README.md docs/TROUBLESHOOTING.md docs/superpowers/specs/2026-10-31-docker-ssh-contexts-design.md
git commit -m "docs: importing ssh docker contexts"
```
