#!/usr/bin/env sh
# Record real Docker Engine API responses for a running sandbox into
# fixtures/docker/. Env values are replaced with REDACTED. Review mount paths
# before committing.
#
# Usage: scripts/record-docker-fixtures.sh [docker socket]
set -eu

SOCKET="${1:-/var/run/docker.sock}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/fixtures/docker"
API="http://localhost/v1.47"

req() { curl -sS --unix-socket "$SOCKET" "$API$1"; }

FILTER='%7B%22label%22%3A%5B%22sandbox.project%22%5D%7D'
req "/containers/json?all=true&filters=$FILTER" \
  | python3 -I -c 'import json,sys; print(json.dumps(json.load(sys.stdin), indent=2))' \
  > "$OUT/containers-list.json"

FIRST=$(python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d[0]["Id"] if d else "")' "$OUT/containers-list.json")
if [ -z "$FIRST" ]; then
  echo "no sandbox containers found; start one with 'sandbox run' first" >&2
  exit 1
fi

req "/containers/$FIRST/json" | python3 -I -c '
import json, sys
d = json.load(sys.stdin)
d["Config"]["Env"] = [e.split("=", 1)[0] + "=REDACTED" for e in d["Config"].get("Env", [])]
print(json.dumps(d, indent=2))' > "$OUT/container-inspect.json"

echo "recorded fixtures into $OUT (review before committing)"
