# Docker Engine API fixtures

JSON responses used by `crates/sandbox-core/tests/docker_fixtures.rs`.

These files were written by hand from the Docker Engine API v1.47 schema and
sandbox 0.73.1's container conventions, because the machine that created
them had no Docker. Replace them with real recordings by running
`scripts/record-docker-fixtures.sh` on a machine with Docker and at least one
running sandbox. Review for secrets before committing: `Config.Env` is
redacted to names by the script, but mounts contain your paths.

- `containers-list.json`: `GET /containers/json?all=true&filters={"label":["sandbox.project"]}`
- `container-inspect.json`: `GET /containers/{id}/json` for the first entry
- `containers-list-with-foreign.json`: list that also contains a non-sandbox
  container, which must be ignored
