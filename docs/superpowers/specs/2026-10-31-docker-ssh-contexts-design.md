# Import ssh:// Docker contexts as remote hosts: design

Date: 2026-10-31. Target release: 0.7.0. Status: approved; implemented on `feat/0.7.0`.

## Goal

Users who already reach a remote machine through a Docker context with an `ssh://` endpoint
should not have to type the host again. Settings, Remote hosts, lists those contexts and adds
the selected ones as ordinary remote hosts.

Success means: on a machine with `ssh://` Docker contexts, the Remote hosts section shows each
of them; importable ones are added with one click and then behave exactly like a host added by
hand; contexts that cannot become an alias say why and offer a `~/.ssh/config` snippet.

## Decisions

| Question                       | Decision                                                                                                       |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| How contexts become hosts      | Explicit import. The app only connects to hosts the user added (AGENTS.md); listing a context never opens SSH. |
| Contexts that are not an alias | Listed, disabled, with the reason and a copyable `~/.ssh/config` snippet. No port field, no transport change.  |
| Source of contexts             | `docker context ls --format '{{json .}}'` through `CommandRunner`.                                             |
| Relationship after import      | One-time copy. Later changes or removal of the context do not touch the host.                                  |

Rejected alternatives:

- Watching `ssh://` contexts automatically (opt-in or always on): connects to hosts the user
  did not add in the app and needs a second host source in the live runtime.
- A `port` field on `RemoteHost`: changes settings, all four `ssh` argument builders and the
  container key for a case `~/.ssh/config` already covers.
- Reading `~/.docker/contexts/meta/*/meta.json`: undocumented format, and `DOCKER_CONFIG` would
  have to be honoured by hand.

## Architecture

### Core: `endpoints.rs`

```rust
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
    /// A `~/.ssh/config` block that gives the context an alias. Set when `alias` is `None`
    /// and the endpoint names a usable host and port.
    pub snippet: Option<String>,
}

pub async fn docker_ssh_contexts(runner: &CommandRunner) -> Vec<SshContext>;
```

- Runs `docker context ls --format '{{json .}}'` and parses one JSON object per line, reading
  `Name` and `DockerEndpoint`. Lines that do not parse are skipped.
- Keeps only endpoints starting with `ssh://`. `unix://`, `tcp://` and `npipe://` are ignored.
- Endpoint rules, after stripping `ssh://`:
  - `user@host` or `host`, with no port, no path (a single trailing `/` is allowed) and passing
    `validate_alias`: `alias = Some(..)`.
  - With a port: reason `uses port <n>`.
  - With a path: reason `has a path`.
  - Failing `validate_alias`: the validation message as reason.
  - Empty host: reason `has no host`, no snippet. Port 0 or not a number: reason
    `has an invalid port`, no snippet.
- Snippet for non-importable contexts, with `<name>` sanitised to `[A-Za-z0-9._-]` (other
  characters become `-`, leading `-` removed, `docker-context` if the result is empty, `docker-local` if it is `local`):

  ```
  Host <name>
    HostName <host>
    User <user>        # only when the endpoint has a user
    Port <port>        # only when the endpoint has a port
  ```

- `docker` missing, non-zero exit or empty output: empty list, no error.
- IPv6 literals in brackets are host values; they fail `validate_alias` and get a snippet.

### Tauri: `src-tauri`

- `list_docker_ssh_contexts() -> Vec<SshContext>`. Read-only; uses `CommandRunner::Local`;
  demo data in mock mode. The UI marks a context as added when its alias is in the Settings
  draft, because Settings saves from its footer.
- On Windows the command returns an empty list.
- Importing uses the existing remote host save path with
  `RemoteHost { alias, display_name: Some(name), enabled: true }`. The live runtime is unchanged.

### UI: `RemoteHostsSection.tsx`

- A "From Docker contexts" list, loaded once when the section mounts.
- Row states:
  - importable: checkbox, context name, endpoint;
  - added: no checkbox, "added";
  - not importable: disabled, the reason, and a copy button for the snippet.
- "Add selected" adds all ticked contexts at once.
- Hidden when the list is empty, which includes Windows.
- `src/types.ts` mirrors `SshContext`. Mock mode returns one importable context
  (`ssh://dev@staging`) and one with a port (`ssh://me@build-vm:2222`).

## Testing

- Core unit tests with `CommandRunner::Fake`: mixed endpoint kinds; `user@host`, bare `host`,
  trailing `/`, port, path, invalid characters, IPv6; snippet text with and without user and
  port; name sanitising; `docker` missing and garbage output give an empty list.
- Tauri: `added` matching.
- UI component test (static render via `src/test/render.tsx`) for the three row states and
  the hidden empty state.

## Docs

- README, Remote hosts: importing from Docker contexts.
- `docs/TROUBLESHOOTING.md`: `ssh://` contexts are not watched directly but can be imported.
- `docs/MILESTONES.md`: update the 0.7.0 follow-up item when shipped.

## Out of scope

- Podman `system connection` entries.
- Syncing contexts after import, or removing hosts when a context is deleted.
- Remote hosts on Windows.
