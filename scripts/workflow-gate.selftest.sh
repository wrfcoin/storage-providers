#!/usr/bin/env bash
# Executable predicate controls for the Rust CI gate job (scripts/workflow-gate.jq). Each fixture under
# scripts/gate-fixtures is a jobs response; pass-* must satisfy the predicate, fail-* must not. PREDICATE overrides the
# predicate file (used to prove the controls turn RED against the old inline predicate).
set -u
here=$(cd "$(dirname "$0")" && pwd)
pred=${PREDICATE:-$here/workflow-gate.jq}
command -v jq > /dev/null || { echo 'VOID: jq missing'; exit 2; }
pass=0; fail=0
for f in "$here"/gate-fixtures/*.json; do
  name=$(basename "$f" .json)
  jq -e -f "$pred" "$f" > /dev/null 2>&1; rc=$?
  case "$name" in pass-*) want=0 ;; fail-*) want=1 ;; *) echo "FAIL $name: fixture name must start pass- or fail-"; fail=$((fail + 1)); continue ;; esac
  if [ "$rc" = "$want" ]; then pass=$((pass + 1)); echo "PASS $name (jq rc=$rc)"; else fail=$((fail + 1)); echo "FAIL $name (jq rc=$rc, wanted $want)"; fi
done
echo "workflow gate controls: $pass passed, $fail failed"
[ "$pass" -gt 0 ] && [ "$fail" = 0 ]
