#!/usr/bin/env bash
# Format only Rust files that are touched in the current diff.
#
# Why this script exists:
#   Non-M6 or external-project waves still need a scoped formatter entrypoint
#   so their diffs stay limited to the files they own. MissionD itself is M6
#   formatter-converged; use scripts/rustfmt-missiond.sh when the task owns
#   repository-wide MissionD Rust formatting.
#
# This script formats only the .rs files that appear in the current diff —
# nothing more. Safe to run inside any wave with confidence that the diff
# stays scoped.
#
# Important: this wrapper always passes `--config skip_children=true` so
# touched module roots as traversal anchors cannot recursively descend into
# untouched child modules outside the task's write scope. For that reason,
# workers and operators MUST invoke this script instead of running
# `rustfmt path/to/mod.rs` directly — direct rustfmt on a module root walks
# the whole subtree.
#
# Usage:
#   scripts/cargo-fmt-touched.sh                # format staged + unstaged
#   scripts/cargo-fmt-touched.sh --check        # check only, exit 1 if dirty
#   scripts/cargo-fmt-touched.sh --staged       # format only staged
#   scripts/cargo-fmt-touched.sh --branch main  # diff against branch base
#
# Exits:
#   0  no files touched, OR all touched files clean (or formatted)
#   1  --check mode: at least one file would change
#   2  rustfmt or git invocation failed

set -euo pipefail

MODE="all"          # all | staged | branch
BRANCH=""
CHECK_ONLY=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --check)   CHECK_ONLY=1 ;;
    --staged)  MODE="staged" ;;
    --branch)  MODE="branch"; shift; BRANCH="${1:-}" ;;
    -h|--help) sed -n '2,28p' "$0"; exit 0 ;;
    *)         echo "unknown arg: $1" >&2; exit 1 ;;
  esac
  shift
done

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

case "$MODE" in
  all)
    # Both staged-for-commit and unstaged work-in-progress.
    FILES=$( { git diff --name-only --diff-filter=ACMR
               git diff --cached --name-only --diff-filter=ACMR
               git ls-files --others --exclude-standard
             } | sort -u )
    ;;
  staged)
    FILES=$(git diff --cached --name-only --diff-filter=ACMR)
    ;;
  branch)
    [ -n "$BRANCH" ] || { echo "--branch requires a base ref" >&2; exit 1; }
    FILES=$( { git diff --name-only --diff-filter=ACMR "${BRANCH}...HEAD"
               git diff --name-only --diff-filter=ACMR
               git ls-files --others --exclude-standard
             } | sort -u )
    ;;
esac

# Keep only existing Rust files. (`--diff-filter=ACMR` already excludes deletes,
# but a rename can leave the old path; the `-f` test handles that defensively.)
TOUCHED_RUST_FILES=$(printf '%s\n' "$FILES" \
  | awk '/\.rs$/ { print }' \
  | while read -r f; do [ -f "$f" ] && printf '%s\n' "$f"; done)

RUST_FILES="$TOUCHED_RUST_FILES"

if [ -z "$RUST_FILES" ]; then
  echo "[fmt-touched] no Rust files in diff — nothing to do."
  exit 0
fi

COUNT=$(printf '%s\n' "$RUST_FILES" | wc -l | tr -d ' ')
echo "[fmt-touched] $COUNT Rust file(s) in diff:"
printf '%s\n' "$RUST_FILES" | sed 's/^/  /'

# Resolve rustfmt: prefer toolchain-local (`cargo fmt -- --files-with-diff …`
# style isn't reliable for a file list), so call rustfmt directly with the
# project's edition.  If the project has a rustfmt.toml at the root, rustfmt
# picks it up automatically.
if ! command -v rustfmt >/dev/null 2>&1; then
  echo "[fmt-touched] FAIL: rustfmt not on PATH" >&2
  exit 2
fi

EDITION="2021"
# If workspace Cargo.toml declares an edition, honor it. (Best-effort grep.)
if [ -f "Cargo.toml" ]; then
  ED=$(grep -E '^edition\s*=' Cargo.toml | head -1 \
        | sed -E 's/.*"([0-9]+)".*/\1/' || true)
  [ -n "$ED" ] && EDITION="$ED"
fi

if [ "$CHECK_ONLY" -eq 1 ]; then
  if printf '%s\n' "$RUST_FILES" | xargs rustfmt --edition "$EDITION" --config skip_children=true --check 2>&1; then
    echo "[fmt-touched] all $COUNT file(s) already formatted."
    exit 0
  else
    echo "[fmt-touched] $COUNT file(s) would change. Run without --check to apply." >&2
    exit 1
  fi
fi

printf '%s\n' "$RUST_FILES" | xargs rustfmt --edition "$EDITION" --config skip_children=true
echo "[fmt-touched] formatted $COUNT file(s)."
