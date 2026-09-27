#!/usr/bin/env bash
set -euo pipefail
# Run in the workspace to validate; selftests supply isolated fixture workspaces.
if ! command -v cargo >/dev/null 2>&1; then
  echo 'STEP cargo RC=2'
  echo 'SUMMARY VOID: cargo is unavailable'
  exit 2
fi
steps=0
run_step() {
  local name=$1 rc=0
  shift
  "$@" || rc=$?
  echo "STEP $name RC=$rc"
  steps=$((steps + 1))
  if (( rc != 0 )); then
    echo "SUMMARY FAIL steps=$steps failed=$name"
    exit 1
  fi
}
run_step fmt cargo fmt --all -- --check
run_step clippy cargo clippy --workspace --all-targets --locked -- -D warnings
test_log=$(mktemp)
trap 'rm -f "$test_log"' EXIT
set +e
cargo test --workspace --locked 2>&1 | tee "$test_log"
pipeline_rc=("${PIPESTATUS[@]}")
set -e
echo "STEP test RC=${pipeline_rc[0]}"
steps=$((steps + 1))
if (( pipeline_rc[0] != 0 || pipeline_rc[1] != 0 )); then
  echo "SUMMARY FAIL steps=$steps failed=test"
  exit 1
fi
tests=$(awk '/^test result: ok\. [0-9]+ passed;/ { total += $4 } END { print total+0 }' "$test_log")
if (( tests == 0 )); then
  echo "SUMMARY VOID steps=$steps tests=0"
  exit 2
fi
if [[ -f scripts/fresh-clone-build.sh ]]; then
  run_step fresh-clone bash scripts/fresh-clone-build.sh
fi
echo "SUMMARY PASS steps=$steps tests=$tests"
