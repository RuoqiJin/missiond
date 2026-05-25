#!/usr/bin/env bash
# MissionD blue-green redeploy: build daemon+MCP -> candidate release -> smoke
# -> active symlink switch -> kickstart -> smoke -> rollback/cleanup.
#
# Usage:
#   scripts/deploy-daemon.sh                  # build + blue-green deploy + smoke
#   scripts/deploy-daemon.sh --build-only     # build, do not touch running daemon
#   scripts/deploy-daemon.sh --no-smoke       # skip post-restart smoke check
#   scripts/deploy-daemon.sh --debug          # use debug profile (faster build)
#   scripts/deploy-daemon.sh --fast           # dev-only shortcut: debug profile + timing + sccache if enabled
#   scripts/deploy-daemon.sh --cleanup-only   # dry-run release cleanup only
#   scripts/deploy-daemon.sh --cleanup-only --apply-cleanup
#
# Environment overrides:
#   MISSIOND_INSTALL_ROOT       release root, default: ~/.xjp-mission
#   MISSIOND_BIN_PATH           stable daemon entrypoint, default: $root/missiond
#   MISSIOND_MCP_BIN_PATH       stable MCP entrypoint, default: $root/mission-mcp
#   MISSIOND_ACTIVE_LINK        active symlink, default: $root/active
#   MISSIOND_RELEASES_DIR       releases dir, default: $root/releases
#   MISSIOND_RELEASE_KEEP       number of newest releases to keep, default: 5
#   MISSIOND_BACKUP_RETENTION_DAYS  old .bak/.new cleanup age, default: 7
#   MISSIOND_SOCKET_PATH        IPC socket, default: ~/.missiond/missiond.sock
#   MISSIOND_LAUNCHCTL_LABEL    launchd label, default: com.missiond.daemon
#   MISSIOND_LAUNCHD_PLIST      launchd plist, default: ~/Library/LaunchAgents/$label.plist
#   MISSIOND_LAUNCHD_PROJECT_ROOT  project root written into launchd, default: current git root
#   MISSIOND_RUNTIME_DIR        runtime artifact root, default: ~/.missiond/runtime/<repo-name>
#   MISSIOND_DEPLOY_TIMEOUT     socket readiness timeout, default: 30
#   MISSIOND_DEPLOY_SMOKE_TIMEOUT  MCP smoke timeout, default: 30
#   MISSIOND_APPLY_BACKUP_CLEANUP  delete old .bak/.new files when cleanup applies, default: 0
#   MISSIOND_USE_SCCACHE        when 1 and sccache exists, export RUSTC_WRAPPER=sccache
#   CARGO_INCREMENTAL           defaults to 0 for deploy builds to avoid filling
#                               disk with target/debug/incremental query caches
#
# Exit codes:
#   0 success
#   1 precondition failure
#   2 build failure
#   3 release install/codesign/pre-switch smoke failure
#   4 launchctl failure
#   5 socket did not become ready
#   6 post-switch smoke failed; rollback attempted

set -euo pipefail

PROFILE="release"
DO_DEPLOY=1
DO_SMOKE=1
CLEANUP_ONLY=0
APPLY_CLEANUP=0
FAST_MODE=0

for arg in "$@"; do
  case "$arg" in
    --build-only) DO_DEPLOY=0; DO_SMOKE=0 ;;
    --no-smoke) DO_SMOKE=0 ;;
    --debug) PROFILE="debug" ;;
    --fast) PROFILE="debug"; FAST_MODE=1 ;;
    --cleanup-only) CLEANUP_ONLY=1; DO_DEPLOY=0; DO_SMOKE=0 ;;
    --apply-cleanup) APPLY_CLEANUP=1 ;;
    -h|--help) sed -n '2,34p' "$0"; exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 1 ;;
  esac
done

INSTALL_ROOT="${MISSIOND_INSTALL_ROOT:-${HOME}/.xjp-mission}"
RELEASES_DIR="${MISSIOND_RELEASES_DIR:-${INSTALL_ROOT}/releases}"
ACTIVE_LINK="${MISSIOND_ACTIVE_LINK:-${INSTALL_ROOT}/active}"
BIN_PATH="${MISSIOND_BIN_PATH:-${INSTALL_ROOT}/missiond}"
MCP_BIN_PATH="${MISSIOND_MCP_BIN_PATH:-${INSTALL_ROOT}/mission-mcp}"
SOCK_PATH="${MISSIOND_SOCKET_PATH:-${HOME}/.missiond/missiond.sock}"
LABEL="${MISSIOND_LAUNCHCTL_LABEL:-com.missiond.daemon}"
LAUNCHD_PLIST="${MISSIOND_LAUNCHD_PLIST:-${HOME}/Library/LaunchAgents/${LABEL}.plist}"
TIMEOUT="${MISSIOND_DEPLOY_TIMEOUT:-30}"
SMOKE_TIMEOUT="${MISSIOND_DEPLOY_SMOKE_TIMEOUT:-30}"
RELEASE_KEEP="${MISSIOND_RELEASE_KEEP:-5}"
BACKUP_RETENTION_DAYS="${MISSIOND_BACKUP_RETENTION_DAYS:-7}"
APPLY_BACKUP_CLEANUP="${MISSIOND_APPLY_BACKUP_CLEANUP:-0}"

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"
LAUNCHD_PROJECT_ROOT="${MISSIOND_LAUNCHD_PROJECT_ROOT:-$REPO_ROOT}"
REPO_ID="$(basename "$REPO_ROOT")"
RUNTIME_DIR="${MISSIOND_RUNTIME_DIR:-${HOME}/.missiond/runtime/${REPO_ID}}"
COMPILED_RUNTIME_DIR="${MISSIOND_COMPILED_RUNTIME_DIR:-${RUNTIME_DIR}/compiled}"
export MISSIOND_RUNTIME_DIR="$RUNTIME_DIR"
export MISSIOND_COMPILED_RUNTIME_DIR="$COMPILED_RUNTIME_DIR"

augment_managed_node_path() {
  local candidates=(
    "${HOME}/.local/share/node-v24.14.0-darwin-arm64/bin"
    "${HOME}/.local/opt/node-v22.13.1-darwin-arm64/bin"
    "${HOME}/.opam/missiond/bin"
    "${HOME}/.opam/default/bin"
    "${HOME}/.local/bin"
    "/opt/homebrew/bin"
    "/usr/local/bin"
  )
  local dir
  for dir in "${candidates[@]}"; do
    if [ -d "$dir" ]; then
      case ":$PATH:" in
        *":$dir:"*) ;;
        *) PATH="$dir:$PATH" ;;
      esac
    fi
  done
  export PATH
}

augment_managed_node_path

case "$PROFILE" in
  release)
    BUILD_ARG="--release"
    ARTIFACT="$REPO_ROOT/target/release/missiond"
    MCP_ARTIFACT="$REPO_ROOT/target/release/mission-mcp"
    ;;
  debug)
    BUILD_ARG=""
    ARTIFACT="$REPO_ROOT/target/debug/missiond"
    MCP_ARTIFACT="$REPO_ROOT/target/debug/mission-mcp"
    ;;
  *) echo "unsupported profile: $PROFILE" >&2; exit 1 ;;
esac

log() { printf '[deploy-daemon] %s\n' "$*" >&2; }
fail() { printf '[deploy-daemon] FAIL: %s\n' "$*" >&2; exit "${2:-1}"; }

TIMING_NAMES=()
TIMING_SECS=()

record_timing() {
  local name="$1"
  local start="$2"
  local elapsed=$(( $(date +%s) - start ))
  TIMING_NAMES+=("$name")
  TIMING_SECS+=("$elapsed")
  log "timing: ${name}=${elapsed}s"
}

print_timing_summary() {
  [ "${#TIMING_NAMES[@]}" -gt 0 ] || return 0
  log "timing-summary: profile=$PROFILE fast=$FAST_MODE"
  local i
  for i in "${!TIMING_NAMES[@]}"; do
    log "timing-summary: ${TIMING_NAMES[$i]}=${TIMING_SECS[$i]}s"
  done
}

codesign_or_verify() {
  local bin="$1"
  if codesign --verify --verbose=2 "$bin" >/dev/null 2>&1; then
    log "codesign: existing signature verified for $(basename "$bin")"
    return 0
  fi
  local output
  if output="$(codesign --force --sign - "$bin" 2>&1)"; then
    echo "$output" | tail -5
    return 0
  fi
  echo "$output" | tail -5 >&2
  # Rust Mach-O binaries are usually already linker-signed. On some macOS
  # versions, force-signing an already-valid binary can fail with an internal
  # Code Signing subsystem error. A candidate that verifies remains deployable.
  if codesign --verify --verbose=2 "$bin" >/dev/null 2>&1; then
    log "codesign: force-sign failed but verified linker signature for $(basename "$bin")"
    return 0
  fi
  return 1
}

resolve_link_target() {
  local link="$1"
  local target
  if [ ! -L "$link" ]; then
    return 1
  fi
  target="$(readlink "$link")"
  case "$target" in
    /*) printf '%s\n' "$target" ;;
    *) printf '%s\n' "$(cd "$(dirname "$link")" && cd "$(dirname "$target")" && pwd -P)/$(basename "$target")" ;;
  esac
}

atomic_symlink_update() {
  local link="$1"
  local target="$2"
  local tmp="${link}.new.$$"
  rm -f "$tmp"
  ln -s "$target" "$tmp"
  if mv -h -f "$tmp" "$link" 2>/dev/null; then
    return 0
  fi
  rm -f "$link"
  mv -f "$tmp" "$link"
}

update_stable_entrypoints() {
  atomic_symlink_update "$BIN_PATH" "$ACTIVE_LINK/bin/missiond"
  atomic_symlink_update "$MCP_BIN_PATH" "$ACTIVE_LINK/bin/mission-mcp"
  log "entrypoints: $BIN_PATH -> $ACTIVE_LINK/bin/missiond"
  log "entrypoints: $MCP_BIN_PATH -> $ACTIVE_LINK/bin/mission-mcp"
}

ensure_default_mcp_config() {
  local config_path="$INSTALL_ROOT/xjp-mcp-config.json"
  if [ -f "$config_path" ]; then
    log "mcp-config: keep existing $config_path"
    return 0
  fi
  mkdir -p "$INSTALL_ROOT"
  cat > "$config_path" <<EOF
{"mcpServers":{"missiond":{"command":"$MCP_BIN_PATH","args":[],"env":{"MISSIOND_SOCKET_PATH":"$SOCK_PATH"}}}}
EOF
  chmod 600 "$config_path"
  log "mcp-config: created default MissionD MCP config $config_path"
}

run_mcp_initialize_smoke() {
  local mcp="$1"
  if command -v timeout >/dev/null 2>&1; then
    timeout 5 "$mcp" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
  elif command -v gtimeout >/dev/null 2>&1; then
    gtimeout 5 "$mcp" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
  elif command -v perl >/dev/null 2>&1; then
    perl -e 'alarm shift @ARGV; exec @ARGV' 5 "$mcp" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
  else
    "$mcp" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-smoke","version":"0"}}}
EOF
  fi
}

wait_for_socket() {
  log "wait: socket $SOCK_PATH (timeout ${TIMEOUT}s)"
  local start elapsed
  start="$(date +%s)"
  while true; do
    if [ -S "$SOCK_PATH" ] && lsof "$SOCK_PATH" 2>/dev/null | grep -q missiond; then
      elapsed=$(( $(date +%s) - start ))
      log "ready: socket bound after ${elapsed}s"
      return 0
    fi
    elapsed=$(( $(date +%s) - start ))
    if [ "$elapsed" -ge "$TIMEOUT" ]; then
      return 1
    fi
    sleep 1
  done
}

kickstart_daemon() {
  if ! launchctl list "$LABEL" >/dev/null 2>&1; then
    log "warn: $LABEL not loaded in launchctl; skipping kickstart."
    return 0
  fi
  log "kickstart: launchctl kickstart -k gui/$(id -u)/$LABEL"
  launchctl kickstart -k "gui/$(id -u)/$LABEL" 2>&1 | tail -5
}

plist_set_or_add_string() {
  local plist="$1"
  local key="$2"
  local value="$3"
  local buddy="/usr/libexec/PlistBuddy"
  if "$buddy" -c "Print :${key}" "$plist" >/dev/null 2>&1; then
    "$buddy" -c "Set :${key} ${value}" "$plist" >/dev/null
  else
    "$buddy" -c "Add :${key} string ${value}" "$plist" >/dev/null
  fi
}

plist_set_or_add_env_string() {
  local plist="$1"
  local key="$2"
  local value="$3"
  local buddy="/usr/libexec/PlistBuddy"
  if ! "$buddy" -c "Print :EnvironmentVariables" "$plist" >/dev/null 2>&1; then
    "$buddy" -c "Add :EnvironmentVariables dict" "$plist" >/dev/null
  fi
  if "$buddy" -c "Print :EnvironmentVariables:${key}" "$plist" >/dev/null 2>&1; then
    "$buddy" -c "Set :EnvironmentVariables:${key} ${value}" "$plist" >/dev/null
  else
    "$buddy" -c "Add :EnvironmentVariables:${key} string ${value}" "$plist" >/dev/null
  fi
}

ensure_launchd_runtime_root() {
  if [ ! -f "$LAUNCHD_PLIST" ]; then
    log "launchd: plist not found at $LAUNCHD_PLIST; deploy will only kickstart loaded label if present"
    return 0
  fi
  [ -d "$LAUNCHD_PROJECT_ROOT/.missiond/v3" ] ||
    fail "launchd project root lacks .missiond/v3: $LAUNCHD_PROJECT_ROOT" 1
  command -v plutil >/dev/null 2>&1 || fail "plutil not on PATH; cannot verify launchd plist" 1
  [ -x /usr/libexec/PlistBuddy ] || fail "PlistBuddy missing; cannot update launchd plist" 1

  plist_set_or_add_string "$LAUNCHD_PLIST" "WorkingDirectory" "$LAUNCHD_PROJECT_ROOT"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_PROJECT_ROOT" "$LAUNCHD_PROJECT_ROOT"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_ORCHESTRATOR_ROOT" "$LAUNCHD_PROJECT_ROOT"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_SOCKET_PATH" "$SOCK_PATH"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_RUNTIME_DIR" "$RUNTIME_DIR"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_COMPILED_RUNTIME_DIR" "$COMPILED_RUNTIME_DIR"
  plutil -lint "$LAUNCHD_PLIST" >/dev/null
  log "launchd: runtime root $LAUNCHD_PROJECT_ROOT written to $LAUNCHD_PLIST"
  log "launchd: artifact runtime dir $RUNTIME_DIR written to $LAUNCHD_PLIST"
}

restart_daemon_supervisor() {
  if [ ! -f "$LAUNCHD_PLIST" ]; then
    kickstart_daemon
    return $?
  fi
  ensure_launchd_runtime_root
  log "launchd: reload $LABEL from $LAUNCHD_PLIST"
  launchctl bootout "gui/$(id -u)/$LABEL" >/dev/null 2>&1 || true
  launchctl bootout "gui/$(id -u)" "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
  launchctl bootstrap "gui/$(id -u)" "$LAUNCHD_PLIST" 2>&1 | tail -8
  launchctl kickstart -k "gui/$(id -u)/$LABEL" 2>&1 | tail -5
}

post_switch_smoke() {
  local smoke_start resp elapsed
  if [ "$DO_SMOKE" -eq 0 ]; then
    log "deploy: done (smoke skipped)."
    return 0
  fi
  [ -x "$MCP_BIN_PATH" ] || fail "mission-mcp entrypoint not executable: $MCP_BIN_PATH" 6
  log "smoke: $MCP_BIN_PATH < initialize"
  smoke_start="$(date +%s)"
  resp=""
  while true; do
    resp="$(run_mcp_initialize_smoke "$MCP_BIN_PATH" 2>&1 | tail -3 || true)"
    if echo "$resp" | grep -q '"protocolVersion"'; then
      log "smoke: IPC responded OK"
      return 0
    fi
    elapsed=$(( $(date +%s) - smoke_start ))
    [ "$elapsed" -lt "$SMOKE_TIMEOUT" ] || break
    log "smoke: IPC not ready yet; retrying..."
    sleep 1
  done
  log "smoke: IPC did not respond cleanly -- output below"
  echo "$resp" | sed 's/^/[smoke] /' >&2
  return 1
}

create_legacy_release_if_needed() {
  local previous
  if previous="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null)"; then
    [ -d "$previous" ] && { printf '%s\n' "$previous"; return 0; }
  fi
  if [ -x "$BIN_PATH" ] && [ ! -L "$BIN_PATH" ] && [ -x "$MCP_BIN_PATH" ] && [ ! -L "$MCP_BIN_PATH" ]; then
    local id dir daemon_hash mcp_hash
    id="legacy-$(date -u +%Y%m%dT%H%M%SZ)"
    dir="$RELEASES_DIR/$id"
    mkdir -p "$dir/bin"
    cp "$BIN_PATH" "$dir/bin/missiond"
    cp "$MCP_BIN_PATH" "$dir/bin/mission-mcp"
    chmod +x "$dir/bin/missiond" "$dir/bin/mission-mcp"
    daemon_hash="$(shasum -a 256 "$dir/bin/missiond" | cut -d' ' -f1)"
    mcp_hash="$(shasum -a 256 "$dir/bin/mission-mcp" | cut -d' ' -f1)"
    cat > "$dir/release-manifest.json" <<EOF
{"schema":"missiond.release-manifest.v1","release_id":"$id","profile":"legacy","git_sha":"unknown","daemon_sha256":"$daemon_hash","mcp_sha256":"$mcp_hash","created_at":"$(date -u +%Y-%m-%dT%H:%M:%SZ)","source":"legacy-installed-binaries"}
EOF
    log "legacy release captured: $dir"
    printf '%s\n' "$dir"
    return 0
  fi
  return 1
}

switch_active_release() {
  local dir="$1"
  atomic_symlink_update "$ACTIVE_LINK" "$dir"
  update_stable_entrypoints
  log "active: $ACTIVE_LINK -> $dir"
}

rollback_to_previous() {
  local previous="$1"
  if [ -z "$previous" ] || [ ! -d "$previous" ]; then
    log "rollback: no previous release available"
    return 1
  fi
  log "rollback: switching active back to $previous"
  switch_active_release "$previous"
  restart_daemon_supervisor >/dev/null 2>&1 || true
  return 0
}

release_complete() {
  local dir="$1"
  [ -f "$dir/release-manifest.json" ] &&
    [ -x "$dir/bin/missiond" ] &&
    [ -x "$dir/bin/mission-mcp" ]
}

typed_lisp_runtime_manifest_json() {
  node <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const compiledDir = process.env.MISSIOND_COMPILED_RUNTIME_DIR
  || (process.env.MISSIOND_RUNTIME_DIR
    ? path.join(process.env.MISSIOND_RUNTIME_DIR, 'compiled')
    : '.missiond/v3/runtime/compiled');
const targets = {
  v3: 'compiled-v3-blueprint.json',
  runtimeConfig: 'compiled-runtime-config.json',
  universe: 'compiled-project-universe.json',
  workflows: 'compiled-workflows.json',
};
const projections = {};
for (const [id, file] of Object.entries(targets)) {
  const rel = path.join(compiledDir, file);
  const raw = fs.readFileSync(rel);
  const json = JSON.parse(raw.toString('utf8'));
  projections[id] = {
    file,
    schema_version: json.schema_version,
    source_hash: json.source_hash,
    file_sha256: crypto.createHash('sha256').update(raw).digest('hex'),
  };
}
process.stdout.write(JSON.stringify({ compiled_dir: compiledDir, projections }));
NODE
}

cleanup_old_releases() {
  local apply="$1"
  mkdir -p "$RELEASES_DIR"
  local active previous newest keep_paths
  active="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null || true)"
  previous="${PREVIOUS_ACTIVE:-}"
  newest="$(find "$RELEASES_DIR" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -r | head -n "$RELEASE_KEEP" || true)"
  keep_paths="$active
$previous
$newest"

  log "cleanup: mode=$([ "$apply" -eq 1 ] && echo apply || echo dry-run), keep_newest=$RELEASE_KEEP"
  find "$RELEASES_DIR" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort | while IFS= read -r dir; do
    if [ "$dir" != "$active" ] && [ "$dir" != "$previous" ] && ! release_complete "$dir"; then
      if [ "$apply" -eq 1 ]; then
        rm -rf "$dir"
        log "cleanup: removed incomplete release $dir"
      else
        log "cleanup: would remove incomplete release $dir"
      fi
      continue
    fi
    case "
$keep_paths
" in
      *"
$dir
"*) log "cleanup: keep release $dir" ;;
      *)
        if [ "$apply" -eq 1 ]; then
          rm -rf "$dir"
          log "cleanup: removed release $dir"
        else
          log "cleanup: would remove release $dir"
        fi
        ;;
    esac
  done

  find "$INSTALL_ROOT" -maxdepth 1 \( -name '*.new.*' -o -name '*.bak.*' \) -mtime +"$BACKUP_RETENTION_DAYS" -print 2>/dev/null | while IFS= read -r file; do
    if [ "$apply" -eq 1 ] && [ "$APPLY_BACKUP_CLEANUP" = "1" ]; then
      rm -rf "$file"
      log "cleanup: removed old backup/temp $file"
    else
      log "cleanup: would remove old backup/temp $file"
    fi
  done
}

mkdir -p "$INSTALL_ROOT" "$RELEASES_DIR" "$(dirname "$SOCK_PATH")" "$COMPILED_RUNTIME_DIR"

if [ "$CLEANUP_ONLY" -eq 1 ]; then
  PREVIOUS_ACTIVE="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null || true)"
  cleanup_old_releases "$APPLY_CLEANUP"
  exit 0
fi

command -v cargo >/dev/null 2>&1 || fail "cargo not on PATH" 1
command -v node >/dev/null 2>&1 || fail "node not on PATH; typed Lisp runtime compile cannot run" 1
command -v dune >/dev/null 2>&1 || fail "dune not on PATH; typed Lisp contract compile cannot run" 1
if [ "${MISSIOND_USE_SCCACHE:-0}" = "1" ] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
  log "build: using RUSTC_WRAPPER=$RUSTC_WRAPPER"
elif [ "${MISSIOND_USE_SCCACHE:-0}" = "1" ]; then
  log "build: MISSIOND_USE_SCCACHE=1 but sccache is not installed; continuing without wrapper"
fi
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
log "build: CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
log "typed-lisp: refresh V3 contract ABI"
TYPED_LISP_START="$(date +%s)"
if ! node scripts/project-v3-contracts.mjs --write 2>&1 | tail -30; then
  fail "typed Lisp contract ABI refresh failed" 1
fi
record_timing "typed-lisp-contract-abi" "$TYPED_LISP_START"
log "typed-lisp: compile V3 runtime projections"
TYPED_LISP_START="$(date +%s)"
if ! node scripts/compile-v3-runtime.mjs --json --out-dir "$COMPILED_RUNTIME_DIR" 2>&1 | tail -30; then
  fail "typed Lisp runtime compile failed" 1
fi
record_timing "typed-lisp-runtime-compile" "$TYPED_LISP_START"
TYPED_LISP_RUNTIME_MANIFEST="$(typed_lisp_runtime_manifest_json)" || fail "typed Lisp runtime manifest failed" 1
log "build: cargo build ${BUILD_ARG} -p missiond-daemon -p missiond-mcp"
BUILD_START="$(date +%s)"
if ! cargo build ${BUILD_ARG} -p missiond-daemon -p missiond-mcp 2>&1 | tail -30; then
  fail "cargo build failed" 2
fi
record_timing "cargo-build" "$BUILD_START"
[ -x "$ARTIFACT" ] || fail "expected artifact missing: $ARTIFACT" 2
[ -x "$MCP_ARTIFACT" ] || fail "expected MCP artifact missing: $MCP_ARTIFACT" 2

NEW_HASH="$(shasum -a 256 "$ARTIFACT" | cut -d' ' -f1)"
NEW_MCP_HASH="$(shasum -a 256 "$MCP_ARTIFACT" | cut -d' ' -f1)"
log "build: daemon sha256=${NEW_HASH:0:12}..."
log "build: MCP sha256=${NEW_MCP_HASH:0:12}..."

if [ "$DO_DEPLOY" -eq 0 ]; then
  log "build-only mode -> done."
  print_timing_summary
  exit 0
fi

RELEASE_START="$(date +%s)"
PREVIOUS_ACTIVE="$(create_legacy_release_if_needed || true)"
GIT_SHA="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
RELEASE_ID="${MISSIOND_RELEASE_ID:-$(date -u +%Y%m%dT%H%M%SZ)-${GIT_SHA}-${PROFILE}}"
CANDIDATE_DIR="$RELEASES_DIR/$RELEASE_ID"

[ ! -e "$CANDIDATE_DIR" ] || fail "candidate release already exists: $CANDIDATE_DIR" 1
mkdir -p "$CANDIDATE_DIR/bin"
cp "$ARTIFACT" "$CANDIDATE_DIR/bin/missiond"
cp "$MCP_ARTIFACT" "$CANDIDATE_DIR/bin/mission-mcp"
chmod +x "$CANDIDATE_DIR/bin/missiond" "$CANDIDATE_DIR/bin/mission-mcp"
record_timing "release-copy" "$RELEASE_START"

if command -v codesign >/dev/null 2>&1; then
  CODESIGN_START="$(date +%s)"
  codesign_or_verify "$CANDIDATE_DIR/bin/missiond"
  codesign_or_verify "$CANDIDATE_DIR/bin/mission-mcp"
  record_timing "codesign" "$CODESIGN_START"
fi
xattr -d com.apple.quarantine "$CANDIDATE_DIR/bin/missiond" 2>/dev/null || true
xattr -d com.apple.quarantine "$CANDIDATE_DIR/bin/mission-mcp" 2>/dev/null || true

cat > "$CANDIDATE_DIR/release-manifest.json" <<EOF
{"schema":"missiond.release-manifest.v1","release_id":"$RELEASE_ID","profile":"$PROFILE","git_sha":"$GIT_SHA","daemon_sha256":"$NEW_HASH","mcp_sha256":"$NEW_MCP_HASH","typed_lisp_runtime":$TYPED_LISP_RUNTIME_MANIFEST,"created_at":"$(date -u +%Y-%m-%dT%H:%M:%SZ)","source":"scripts/deploy-daemon.sh"}
EOF
log "candidate: $CANDIDATE_DIR"

log "pre-switch smoke: candidate MCP initialize"
PRE_SWITCH_SMOKE_START="$(date +%s)"
PRE_RESP="$(run_mcp_initialize_smoke "$CANDIDATE_DIR/bin/mission-mcp" 2>&1 | tail -3 || true)"
if ! echo "$PRE_RESP" | grep -q '"protocolVersion"'; then
  echo "$PRE_RESP" | sed 's/^/[pre-smoke] /' >&2
  fail "candidate MCP initialize failed before active switch" 3
fi
record_timing "pre-switch-mcp-smoke" "$PRE_SWITCH_SMOKE_START"

switch_active_release "$CANDIDATE_DIR"
ensure_default_mcp_config

KICKSTART_START="$(date +%s)"
if ! restart_daemon_supervisor; then
  rollback_to_previous "$PREVIOUS_ACTIVE" || true
  fail "launchctl reload/kickstart failed; rollback attempted" 4
fi
record_timing "launchd-kickstart" "$KICKSTART_START"

SOCKET_WAIT_START="$(date +%s)"
if ! wait_for_socket; then
  rollback_to_previous "$PREVIOUS_ACTIVE" || true
  fail "socket not ready after ${TIMEOUT}s; rollback attempted" 5
fi
record_timing "socket-wait" "$SOCKET_WAIT_START"

RUN_PID="$(lsof -t "$SOCK_PATH" 2>/dev/null | head -1 || true)"
if [ -n "$RUN_PID" ]; then
  RUN_BIN="$(ps -o comm= -p "$RUN_PID" 2>/dev/null || true)"
  log "running: PID=$RUN_PID comm=$RUN_BIN"
fi

POST_SMOKE_START="$(date +%s)"
if ! post_switch_smoke; then
  rollback_to_previous "$PREVIOUS_ACTIVE" || true
  fail "smoke check failed; rollback attempted" 6
fi
record_timing "post-switch-mcp-smoke" "$POST_SMOKE_START"

CLEANUP_START="$(date +%s)"
cleanup_old_releases 1
record_timing "cleanup" "$CLEANUP_START"
print_timing_summary
log "deploy: done. active_release=$RELEASE_ID previous=${PREVIOUS_ACTIVE:-none}"
