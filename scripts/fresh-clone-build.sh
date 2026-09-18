#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "VOID: cargo is unavailable" >&2
  exit 2
fi

source_repo=$(git rev-parse --show-toplevel)
empty_parent=$(mktemp -d)
trap 'rm -rf "$empty_parent"' EXIT

git clone --quiet --depth 1 "file://$source_repo" "$empty_parent/sp"
test "$(find "$empty_parent" -mindepth 1 -maxdepth 1 -type d | wc -l)" -eq 1

cd "$empty_parent/sp"
cargo check --workspace --locked
