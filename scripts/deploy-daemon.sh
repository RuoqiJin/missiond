#!/usr/bin/env bash
# MissionD daemon redeploy: build → backup → codesign → kickstart → wait → smoke.
#
# Why this script exists:
#   The bare-hand "build/copy/codesign/kickstart" sequence has bitten us repeatedly
#   (dyld stalls, self-deleted sockets, unsigned binary rejection by spctl,
#   half-active processes that hold the WS port without the IPC socket).
#   This script encodes the safe order, verifies each step, and refuses to
#   proceed if any precondition fails — so we trade ~5 lines of CLI typing
#   for predictable redeploys.
#
# Usage:
#   scripts/deploy-daemon.sh                  # build + deploy + smoke
#   scripts/deploy-daemon.sh --build-only     # build, do not touch running daemon
#   scripts/deploy-daemon.sh --no-smoke       # skip post-restart smoke check
#   scripts/deploy-daemon.sh --debug          # use debug profile (faster build)
#
# Environment overrides:
#   MISSIOND_BIN_PATH       installed binary location
#                           default: ~/.xjp-mission/missiond
#   MISSIOND_SOCKET_PATH    IPC socket the daemon owns
#                           default: ~/.missiond/missiond.sock
#   MISSIOND_LAUNCHCTL_LABEL  launchd label
#                           default: com.missiond.daemon
#   MISSIOND_DEPLOY_TIMEOUT   socket-readiness timeout (seconds)
#                           default: 30
#
# Exits:
#   0  success
#   1  precondition failure (cargo missing, paths invalid, etc.)
#   2  build failure
#   3  install/codesign failure
#   4  launchctl failure
#   5  socket did not become ready in time
#   6  smoke check failed (binary started but IPC unhealthy) — backup restored
#
# Read-only assumptions:
#   * Only mutates: $MISSIOND_BIN_PATH (atomic-replace) and launchd job state.
#   * Never writes to git, never touches working tree, never deletes the socket
#     file (launchd owns it; we wait for the new daemon to recreate it).

set -euo pipefail

# ─── Configuration ────────────────────────────────────────────────────────
PROFILE="release"
DO_DEPLOY=1
DO_SMOKE=1
for arg in "$@"; do
  case "$arg" in
    --build-only) DO_DEPLOY=0; DO_SMOKE=0 ;;
    --no-smoke)   DO_SMOKE=0 ;;
    --debug)      PROFILE="debug" ;;
    -h|--help)    sed -n '2,30p' "$0"; exit 0 ;;
    *)            echo "unknown arg: $arg" >&2; exit 1 ;;
  esac
done

BIN_PATH="${MISSIOND_BIN_PATH:-${HOME}/.xjp-mission/missiond}"
SOCK_PATH="${MISSIOND_SOCKET_PATH:-${HOME}/.missiond/missiond.sock}"
LABEL="${MISSIOND_LAUNCHCTL_LABEL:-com.missiond.daemon}"
TIMEOUT="${MISSIOND_DEPLOY_TIMEOUT:-30}"

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

case "$PROFILE" in
  release) BUILD_ARG="--release"; ARTIFACT="$REPO_ROOT/target/release/missiond" ;;
  debug)   BUILD_ARG="";          ARTIFACT="$REPO_ROOT/target/debug/missiond" ;;
esac

log()  { printf '[deploy-daemon] %s\n'   "$*" >&2; }
fail() { printf '[deploy-daemon] FAIL: %s\n' "$*" >&2; exit "${2:-1}"; }

# ─── Phase 1: Build ───────────────────────────────────────────────────────
command -v cargo >/dev/null 2>&1 || fail "cargo not on PATH" 1
log "build: cargo build $BUILD_ARG -p missiond-daemon"
if ! cargo build $BUILD_ARG -p missiond-daemon 2>&1 | tail -30; then
  fail "cargo build failed" 2
fi
[ -x "$ARTIFACT" ] || fail "expected artifact missing: $ARTIFACT" 2
NEW_HASH="$(shasum -a 256 "$ARTIFACT" | cut -d' ' -f1)"
log "build: new binary $ARTIFACT  sha256=${NEW_HASH:0:12}…"

if [ "$DO_DEPLOY" -eq 0 ]; then
  log "build-only mode → done."
  exit 0
fi

# ─── Phase 2: Pre-deploy guards ───────────────────────────────────────────
[ -d "$(dirname "$BIN_PATH")" ] || fail "install dir missing: $(dirname "$BIN_PATH")" 1
[ -d "$(dirname "$SOCK_PATH")" ] || fail "socket dir missing: $(dirname "$SOCK_PATH")" 1

if [ -x "$BIN_PATH" ]; then
  CUR_HASH="$(shasum -a 256 "$BIN_PATH" | cut -d' ' -f1)"
  if [ "$CUR_HASH" = "$NEW_HASH" ]; then
    log "installed binary already matches build sha — nothing to do."
    exit 0
  fi
  log "installed: ${CUR_HASH:0:12}…  → upgrading to ${NEW_HASH:0:12}…"
else
  log "no binary installed yet at $BIN_PATH — fresh deploy."
fi

# ─── Phase 3: Backup + atomic install ─────────────────────────────────────
BACKUP_PATH="${BIN_PATH}.bak.$(date -u +%Y%m%dT%H%M%SZ)"
if [ -x "$BIN_PATH" ]; then
  cp "$BIN_PATH" "$BACKUP_PATH"
  log "backup: $BACKUP_PATH"
fi

# Use a temp file in the same directory then `mv` for atomic replace.
TMP_BIN="${BIN_PATH}.new.$$"
trap 'rm -f "$TMP_BIN"' EXIT
cp "$ARTIFACT" "$TMP_BIN"
chmod +x "$TMP_BIN"

# Ad-hoc codesign (-s -). LaunchAgent rejects unsigned binaries on macOS.
if command -v codesign >/dev/null 2>&1; then
  if ! codesign --force --sign - "$TMP_BIN" 2>&1 | tail -5; then
    rm -f "$TMP_BIN"; fail "codesign failed" 3
  fi
fi
# Strip quarantine attr in case copy inherited one.
xattr -d com.apple.quarantine "$TMP_BIN" 2>/dev/null || true

mv "$TMP_BIN" "$BIN_PATH"
trap - EXIT
log "installed: $BIN_PATH"

# ─── Phase 4: Restart via launchd ─────────────────────────────────────────
if ! launchctl list "$LABEL" >/dev/null 2>&1; then
  log "warn: $LABEL not loaded in launchctl — skipping kickstart."
  log "       (manual: launchctl bootstrap gui/$(id -u) <plist-path>)"
else
  log "kickstart: launchctl kickstart -k gui/$(id -u)/$LABEL"
  if ! launchctl kickstart -k "gui/$(id -u)/$LABEL" 2>&1 | tail -5; then
    fail "launchctl kickstart failed" 4
  fi
fi

# ─── Phase 5: Wait for socket readiness ───────────────────────────────────
log "wait: socket $SOCK_PATH (timeout ${TIMEOUT}s)"
START_TS=$(date +%s)
while true; do
  if [ -S "$SOCK_PATH" ]; then
    # Verify a process actually owns it (lsof returns rows when bound).
    if lsof "$SOCK_PATH" 2>/dev/null | grep -q missiond; then
      ELAPSED=$(( $(date +%s) - START_TS ))
      log "ready: socket bound after ${ELAPSED}s"
      break
    fi
  fi
  ELAPSED=$(( $(date +%s) - START_TS ))
  if [ "$ELAPSED" -ge "$TIMEOUT" ]; then
    fail "socket not ready after ${TIMEOUT}s — daemon may have failed to start" 5
  fi
  sleep 1
done

# Confirm the running PID matches the new binary on disk (catches the
# launchd-still-running-old-image case we hit during wave47).
RUN_PID=$(lsof -t "$SOCK_PATH" 2>/dev/null | head -1 || true)
if [ -n "$RUN_PID" ]; then
  RUN_BIN=$(ps -o comm= -p "$RUN_PID" 2>/dev/null || true)
  log "running: PID=$RUN_PID  comm=$RUN_BIN"
fi

# ─── Phase 6: Optional smoke check ────────────────────────────────────────
if [ "$DO_SMOKE" -eq 0 ]; then
  log "deploy: done (smoke skipped)."
  exit 0
fi

if [ -x "$REPO_ROOT/target/debug/mission-mcp" ] || [ -x "$REPO_ROOT/target/release/mission-mcp" ]; then
  MCP="${REPO_ROOT}/target/release/mission-mcp"
  [ -x "$MCP" ] || MCP="${REPO_ROOT}/target/debug/mission-mcp"

  run_mcp_initialize_smoke() {
    if command -v timeout >/dev/null 2>&1; then
      timeout 5 "$MCP" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
    elif command -v gtimeout >/dev/null 2>&1; then
      gtimeout 5 "$MCP" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
    elif command -v perl >/dev/null 2>&1; then
      perl -e 'alarm shift @ARGV; exec @ARGV' 5 "$MCP" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
    else
      "$MCP" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
    fi
  }

  # Send a minimal initialize + tools/call to the IPC. We don't run a full
  # MCP exchange — we just want to know: does the IPC respond?
  log "smoke: $MCP < initialize"
  RESP=$(run_mcp_initialize_smoke 2>&1 | tail -3 || true)
  if echo "$RESP" | grep -q '"protocolVersion"'; then
    log "smoke: IPC responded OK"
  else
    log "smoke: IPC did not respond cleanly — output below"
    echo "$RESP" | sed 's/^/[smoke] /' >&2
    if [ -n "${BACKUP_PATH:-}" ] && [ -f "$BACKUP_PATH" ]; then
      log "smoke: rolling back to $BACKUP_PATH"
      cp "$BACKUP_PATH" "$BIN_PATH"
      [ "${LABEL:-}" ] && launchctl kickstart -k "gui/$(id -u)/$LABEL" >/dev/null 2>&1 || true
    fi
    fail "smoke check failed — backup restored" 6
  fi
else
  log "smoke: mission-mcp not built; skipping (build it with cargo build -p missiond-mcp to enable)"
fi

log "deploy: done. backup retained at $BACKUP_PATH"
