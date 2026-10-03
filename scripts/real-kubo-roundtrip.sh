#!/usr/bin/env bash
# Real-Kubo round trip for storage-providers#78 (and sp#82): add, restart the node, cat, re-import intact
# (same CID) and corrupt (refused). Starts ONE disposable, OFFLINE ipfs/kubo container, binds its API to
# 127.0.0.1 only, keeps its repo under this checkout, and removes exactly that container on exit.
# Never touches any other container; no registry push; no remote peers (the daemon runs --offline).
#   bash scripts/real-kubo-roundtrip.sh
# Needs docker, cargo, curl. The test itself (crates/ipfs/tests/real_kubo_78.rs) is a no-op unless KUBO_API is set.
set -u
IMAGE=${KUBO_IMAGE:-ipfs/kubo:v0.24.0}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
NAME="cb2-kubo-82-$STAMP"
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO="$ROOT/.kubo-82-$STAMP"
CIDFILE="$REPO/cid.txt"
PORT=${KUBO_HOST_PORT:-$((15000 + RANDOM % 2000))}
cleanup() { docker rm -f "$NAME" > /dev/null 2>&1; echo "removed container $NAME (repo kept at $REPO)"; }
trap cleanup EXIT
mkdir -p "$REPO/ipfs" "$REPO/export" || exit 1
echo "== start $NAME ($IMAGE) api=127.0.0.1:$PORT repo=$REPO/ipfs (offline)"
docker run -d --name "$NAME" -p "127.0.0.1:$PORT:5001" -v "$REPO/ipfs:/data/ipfs" -v "$REPO/export:/export" \
  "$IMAGE" daemon --offline > /dev/null || { echo 'STOP: docker run'; exit 1; }
wait_api() {
  for _ in $(seq 1 60); do
    curl -fsS -m 2 -X POST "http://127.0.0.1:$PORT/api/v0/version" 2> /dev/null && { echo; return 0; }
    sleep 1
  done
  echo 'STOP: kubo API did not come up'; docker logs "$NAME" 2>&1 | tail -20; return 1
}
wait_api || exit 1
export KUBO_API="http://127.0.0.1:$PORT" KUBO_CID_FILE="$CIDFILE"
run_phase() { KUBO_PHASE=$1 cargo test -p wrfcoin-ipfs --test real_kubo_78 --locked -- --nocapture --test-threads=1 2>&1 | grep -E 'ADD|CAT|REIMPORT|test result|panicked|error' ; return "${PIPESTATUS[0]}"; }
echo "== phase add"; run_phase add || { echo 'STOP: phase add failed'; exit 1; }
echo "== docker restart $NAME"; docker restart "$NAME" > /dev/null || exit 1
wait_api || exit 1
echo "== phase cat (fresh provider, empty cache; after restart)"; run_phase cat || { echo 'STOP: phase cat failed'; exit 1; }
echo "REAL_KUBO_ROUNDTRIP_OK cid=$(cat "$CIDFILE")"
