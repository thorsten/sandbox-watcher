#!/usr/bin/env sh
# Optional benchmark against a real Docker or Podman: starts N sandbox-like
# containers, runs the real-runtime part of crates/sandbox-core/tests/budgets.rs,
# prints the measured refresh time and removes the containers again.
#
#   scripts/bench-runtime.sh            # 20 containers, Docker
#   scripts/bench-runtime.sh 50         # 50 containers
#   SANDBOX_WATCHER_RUNTIME=podman DOCKER_HOST=unix://... scripts/bench-runtime.sh
#
# The containers are labelled like sandbox's and carry the ready marker so
# the watcher probes them exactly like real ones. Nothing here runs in CI;
# the numbers depend on the machine and the runtime and are for the
# "real runtime" column of docs/PERFORMANCE.md.
set -eu

count="${1:-20}"
runtime="${SANDBOX_WATCHER_RUNTIME:-docker}"
case "$runtime" in
  docker) cli=docker; image=alpine:3 ;;
  podman) cli=podman; image=docker.io/library/alpine:3 ;;
  *) echo "SANDBOX_WATCHER_RUNTIME must be docker or podman" >&2; exit 2 ;;
esac
prefix="sandbox-watcher-bench"

cleanup() {
  "$cli" ps -aq --filter "name=$prefix-" | xargs -r "$cli" rm -f >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM
cleanup

echo "starting $count $runtime containers"
i=0
while [ "$i" -lt "$count" ]; do
  "$cli" run -d --name "$prefix-$i" \
    --label "sandbox.project=bench-$i" --label sandbox.hash=abcdef123456 \
    "$image" sh -c 'mkdir -p /tmp/sandbox-sessions && touch /tmp/.sandbox-ready && sleep 600' >/dev/null
  i=$((i + 1))
done
i=0
while [ "$i" -lt "$count" ]; do
  until "$cli" exec "$prefix-$i" test -f /tmp/.sandbox-ready 2>/dev/null; do sleep 0.2; done
  i=$((i + 1))
done

cd "$(dirname "$0")/.."
SANDBOX_WATCHER_DOCKER_BENCH=1 SANDBOX_WATCHER_BENCH_CONTAINERS="$count" \
  cargo test --release -p sandbox-core --test budgets real_runtime_refresh -- --nocapture
