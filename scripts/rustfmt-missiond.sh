#!/usr/bin/env bash
# Format/check every Rust file owned by this MissionD repository.
#
# This is the M6 formatter-convergence entrypoint. It intentionally formats
# only repo-owned crates/** files so path dependencies such as ../semantic-terminal
# or ../jarvis-forge are not mutated by a MissionD formatting task.
#
# Usage:
#   scripts/rustfmt-missiond.sh
#   scripts/rustfmt-missiond.sh --check

set -euo pipefail

CHECK_ONLY=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --check) CHECK_ONLY=1 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
  shift
done

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if ! command -v rustfmt >/dev/null 2>&1; then
  echo "[rustfmt-missiond] FAIL: rustfmt not on PATH" >&2
  exit 2
fi

EDITION="$(
  grep -E '^[[:space:]]*edition[[:space:]]*=' Cargo.toml \
    | head -1 \
    | sed -E 's/.*"([^"]+)".*/\1/'
)"
if [ -z "$EDITION" ]; then
  echo "[rustfmt-missiond] FAIL: root Cargo.toml has no package edition" >&2
  exit 2
fi

if rg -n 'missiond-rustfmt-exempt' crates --glob '*.rs' >/tmp/missiond-rustfmt-exempt.$$ 2>/dev/null; then
  echo "[rustfmt-missiond] FAIL: rustfmt exemption marker remains in MissionD source:" >&2
  cat /tmp/missiond-rustfmt-exempt.$$ >&2
  rm -f /tmp/missiond-rustfmt-exempt.$$
  exit 1
fi
rm -f /tmp/missiond-rustfmt-exempt.$$

RUST_FILES=$(find crates -name '*.rs' -type f | sort)
if [ -z "$RUST_FILES" ]; then
  echo "[rustfmt-missiond] no Rust files under crates/."
  exit 0
fi

COUNT=$(printf '%s\n' "$RUST_FILES" | wc -l | tr -d ' ')
echo "[rustfmt-missiond] $COUNT MissionD Rust file(s)."
echo "[rustfmt-missiond] edition=$EDITION (from Cargo.toml)."

if [ "$CHECK_ONLY" -eq 1 ]; then
  printf '%s\n' "$RUST_FILES" | xargs rustfmt --edition "$EDITION" --config skip_children=true --check
  echo "[rustfmt-missiond] all MissionD Rust files are formatted."
else
  printf '%s\n' "$RUST_FILES" | xargs rustfmt --edition "$EDITION" --config skip_children=true
  echo "[rustfmt-missiond] formatted MissionD Rust files."
fi
