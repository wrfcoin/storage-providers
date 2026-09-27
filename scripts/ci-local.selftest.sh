#!/usr/bin/env bash
set -euo pipefail
gate=$(cd "$(dirname "$0")" && pwd)/ci-local.sh
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/crate/src" "$scratch/no-cargo"
printf '[package]\nname = "gate-control"\nversion = "0.1.0"\nedition = "2021"\n' > "$scratch/crate/Cargo.toml"
cd "$scratch/crate"
touch src/lib.rs
cargo generate-lockfile --offline
expect() {
  local name=$1 expected=$2 rc=0
  shift 2
  "$@" > "$scratch/$name.log" 2>&1 || rc=$?
  if [[ $rc != "$expected" ]]; then
    cat "$scratch/$name.log"
    echo "FAIL $name expected=$expected actual=$rc"
    exit 1
  fi
  echo "PASS $name RC=$rc"
}
cat > src/lib.rs <<'EOF'
#[test]
fn fails() {
    assert_eq!(1, 2);
}
EOF
expect failing-test 1 bash "$gate"
expect missing-cargo 2 env PATH="$scratch/no-cargo" /bin/bash "$gate"
printf 'pub fn noop() {}\n' > src/lib.rs
expect zero-tests 2 bash "$gate"
cat > src/lib.rs <<'EOF'
#[test]
fn passes() {
    assert_eq!(2 + 2, 4);
}
EOF
expect passing-test 0 bash "$gate"
echo 'Self-test: 4 controls passed'
