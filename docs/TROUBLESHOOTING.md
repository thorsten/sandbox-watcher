# Troubleshooting

Sandbox Watcher finds runtimes the way sandbox does: `DOCKER_HOST` or
`CONTAINER_HOST` first, then the usual sockets, then the `docker`, `podman`
or `container` binary on `PATH`. Most problems come down to the app not
finding a socket, or finding one it is not allowed to use.

## No containers are shown

- Only containers started by sandbox appear. They carry the
  `sandbox.project` label; other containers on the same runtime are ignored.
- Check that the runtime answers in a terminal: `docker ps`, `podman ps` or
  `container list`. If that fails, the app cannot reach it either.
- If more than one runtime is installed and sandbox's `config.toml` sets
  `runtime`, only that runtime is used. Pick a different one under
  Settings → Runtime, or with `SANDBOX_WATCHER_RUNTIME=docker|podman|apple-container`.
  The Settings choice wins over the environment variable.

## Environment variables are ignored

Apps started from the Dock, Finder, the Start menu or a desktop launcher do
not see variables set in `.zshrc` or `.bashrc`, and on macOS they get a
minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`). Binaries installed by
Homebrew or in `/usr/local/bin` may then not be found.

- Start the app once from a terminal to confirm: `open -a "Sandbox Watcher"`
  on macOS inherits the shell's environment.
- On macOS, `launchctl setenv DOCKER_HOST unix:///path/to/docker.sock` makes a
  variable visible to apps started afterwards (until the next login).
- On Linux, set the variable in `~/.config/environment.d/*.conf` (systemd
  sessions) or in the launcher's `Exec=env DOCKER_HOST=… sandbox-watcher` line.

## Docker on Linux: permission denied on the socket

`/var/run/docker.sock` is owned by `root:docker`. Add your user to the
`docker` group and log in again:

```sh
sudo usermod -aG docker "$USER"
```

Membership in the `docker` group is equivalent to root on that machine.
Rootless Docker avoids this; see below.

## Rootless Docker

Rootless Docker listens on `$XDG_RUNTIME_DIR/docker.sock`, which is not one
of the default paths. Point the app at it:

```sh
export DOCKER_HOST=unix://$XDG_RUNTIME_DIR/docker.sock
```

`docker context use rootless` also works when only the `docker` binary is
found: the app then reads the host of the active context.

## Docker Desktop, Colima, Rancher Desktop

On macOS the app checks `/var/run/docker.sock`, `~/.docker/run/docker.sock`,
`~/.colima/default/docker.sock` and `~/.rd/docker.sock`; on Linux
`/var/run/docker.sock` and `~/.docker/desktop/docker.sock`. Colima profiles
other than `default` need `DOCKER_HOST` or an active `docker context`.
Contexts that use `ssh://` are not supported.

## Rootless Podman on Linux

The API socket is not running by default. Enable it for your user:

```sh
systemctl --user enable --now podman.socket
```

The app then finds `$XDG_RUNTIME_DIR/podman/podman.sock`. The system socket
`/run/podman/podman.sock` (from `sudo systemctl enable --now podman.socket`)
is used when no rootless socket exists, and needs root or matching group
permissions.

## Podman machine on macOS and Windows

The machine has to run: `podman machine start`. The app asks
`podman machine inspect` for the socket, and on Windows also tries the pipe
`\\.\pipe\podman-machine-default`. For a machine with another name, set
`CONTAINER_HOST`, for example:

```sh
# macOS
export CONTAINER_HOST=unix://$(podman machine inspect mymachine --format '{{.ConnectionInfo.PodmanSocket.Path}}')
```

```powershell
# Windows
$env:CONTAINER_HOST = "npipe:////./pipe/podman-mymachine"
```

## Apple `container`

- Needs an Apple silicon Mac with macOS 26 or later, and the service must run:
  `container system start`.
- The `container` CLI has no event stream and no stats API. The list refreshes
  by polling at the interval set in Settings, and the Resources tab is hidden.
- The `container` binary must be on the app's `PATH`; see
  [Environment variables are ignored](#environment-variables-are-ignored).

## Containers appear, but no sessions or network activity

Sessions and network activity are read by running commands inside the
container (`exec`).

- **No sessions:** the session probe could not run, or the agent was started
  without sandbox's session markers. Check that
  `docker exec <container> ls /tmp/sandbox-sessions` works.
- **No network activity:** the proxy, DNS and firewall logs are read as root
  inside the container with sandbox's container tools. If exec as root is
  denied or the tools are missing (older sandbox images), the Network tab
  stays empty for that container.
- **No agent timeline:** the agent's session files have a format this version
  does not parse yet. Please open an issue with the agent name and version.

## Still stuck

Open an issue at <https://github.com/thorsten/sandbox-watcher/issues> with
your OS, runtime and version, and the output of `docker context ls` or
`podman system connection ls`.
