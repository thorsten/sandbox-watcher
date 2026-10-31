# Security policy

## Supported versions

Only the latest release gets security fixes. Sandbox Watcher is pre-1.0, so
a fix ships as a new release rather than a patch to an older line.

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Report it privately, either:

- through GitHub: **Security → Report a vulnerability** on
  [thorsten/sandbox-watcher](https://github.com/thorsten/sandbox-watcher/security/advisories/new), or
- by email to [thorsten@rinne.info](mailto:thorsten@rinne.info).

Include the version, your OS and container runtime, what you did, what
happened, and what an attacker gains. A proof of concept helps but is not
required.

You will get an answer within a week. Once a fix is released, the advisory
is published and credits you unless you prefer otherwise.

## Scope

In scope is anything in this repository, in particular:

- what the app runs: commands inside containers, `ssh` invocations for remote
  hosts, and the editor, terminal and file-manager launchers;
- the allow-list edit, the only change the app makes to sandbox's config;
- paths the app derives from container mounts and opens on the host;
- the update check and the verification of downloaded updates;
- anything that reads environment variable values or sends data off the
  machine, which the app must never do.

Problems in [MaibornWolff Sandbox](https://github.com/MaibornWolff/sandbox)
itself, Docker, Podman, Apple `container` or OpenSSH belong to those projects.
If you are unsure where a problem lives, report it here and it will be
forwarded.
