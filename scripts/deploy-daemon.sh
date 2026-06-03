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
#   MISSIOND_DEPLOY_LOCK_PATH   deploy ownership lock directory, default:
#                               $root/deploy.lock.d
#   MISSIOND_DEPLOY_LOCK_STALE_SECS  age before metadata-less lock recovery,
#                               default: 300
#   MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT  expected launchd_project_root for
#                               the currently active release before mutating
#                               active/apply-cleanup; default: this Git root
#   MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER  explicit 1/true override for
#                               switching active from another project root
#   MISSIOND_BACKUP_RETENTION_DAYS  old .bak/.new cleanup age, default: 7
#   MISSIOND_SOCKET_PATH        IPC socket, default: ~/.missiond/missiond.sock
#   MISSIOND_LAUNCHCTL_LABEL    launchd label, default: com.missiond.daemon
#   MISSIOND_LAUNCHD_PLIST      launchd plist, default: ~/Library/LaunchAgents/$label.plist
#   MISSIOND_LAUNCHD_PROJECT_ROOT  deploy owner root, default: current git root.
#                               launchd runtime uses a release-local source
#                               snapshot by default; this root is recorded as
#                               release_owner_root for ownership checks.
#   MISSIOND_DEPLOY_OWNER_ROOT  explicit owner root for active mutation guards.
#                               Default: current stable git root, or the active
#                               release manifest's release_owner_root when
#                               deploying from a transient worktree.
#   MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT  expected launchd_project_root for
#                               the currently active release before mutating
#                               active/apply-cleanup; default: deploy owner root
#   MISSIOND_DEPLOY_EXPECTED_ACTIVE_RELEASE  expected active release directory
#                               before active switch; default: active link
#                               observed at script start
#   MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER  explicit 1/true override for
#                               switching active from another project root
#   MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE  explicit 1/true override for
#                               switching active after another deploy changed it
#   MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION  explicit 1/true override for
#                               switching active to a candidate commit that is
#                               not a descendant of the current active release
#                               commit. Required for intentional rollback or
#                               branch-divergence deploys from the same owner root.
#   MISSIOND_RELEASE_SOURCE_SNAPSHOT  when truthy, launchd points to
#                               <release>/source, an immutable git-archive
#                               snapshot matching compiled runtime; default: 1
#   MISSIOND_RELEASE_ALLOW_DIRTY_SOURCE  allow source snapshot while V3/runtime
#                               projection inputs are dirty; default: 0
#   MISSIOND_RUNTIME_DIR        runtime artifact root, default:
#                               ~/.missiond/runtime/<deploy-owner-root-name>
#   MISSIOND_COMPILED_RUNTIME_DIR  compiled runtime projection dir. Deploy
#                               switches launchd to a release-local compiled
#                               dir under the candidate release so failed or
#                               stale deploys cannot poison the running ABI.
#   MISSIOND_CLEAN_REPO_RUNTIME_CACHE  after a successful deploy, prune repo
#                               .missiond/v3/runtime cache when external
#                               runtime dirs are verified, default: 1
#   MISSIOND_INTERACTION_SERVICE_TOKEN  optional service token injected from
#                               secret-store for Jarvis/interaction smoke.
#   MISSIOND_INTERACTION_AUTH_USERINFO_URL  optional Auth userinfo endpoint.
#   MISSIOND_INTERACTION_AUTH_TIMEOUT_MS  optional Auth userinfo timeout.
#   MISSIOND_XJPCODE_WORKER_URL  optional portable xjpcode worker base URL
#                               injected into launchd for MissionD delegated
#                               xjpcode read-only worker dispatch.
#   MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN optional bearer token for internal
#                               provider-box HTTP calls. Generated for launchd
#                               when neither this nor MISSIOND_AGY_INTERNAL_TOKEN
#                               is supplied or already present in launchd.
#   MISSIOND_PROVIDER_BOX_PROXY_BASE_URL optional public base URL for the
#                               managed provider-box proxy, e.g.
#                               https://auth.xiaojinpro.com/tunnel/proxy/rickyhqmac-mini/missiond.
#                               or https://jarvis.xiaojinpro.top/missiond.
#   MISSIOND_AGY_PROVIDER_BOX_BASE_URL optional AGY-specific alias for the
#                               managed provider-box proxy base URL.
#   MISSIOND_JARVIS_DIRECT_ANSWER_PROVIDER optional provider-box provider id.
#                               Defaults to codex_cli.
#   MISSIOND_JARVIS_DIRECT_ANSWER_MODEL optional model passed to the selected
#                               provider-box provider.
#   MISSIOND_JARVIS_DIRECT_ANSWER_TIMEOUT_SECS optional timeout for provider-box
#                               direct-answer calls.
#   MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT  1/0/auto. In auto mode, call the
#                               localhost-only Jarvis slot ensure endpoint
#                               after restart when launchd/current env enables
#                               MISSIOND_JARVIS_SLOT_AUTO_HEAL. Default: auto.
#   MISSIOND_FULL_OS_ENABLE     when truthy, enable optional full-os layers in
#                               launchd. Individual MISSIOND_FEATURE_* gates
#                               are also propagated when present.
#   MISSIOND_PG_MAX_CONNECTIONS / MISSIOND_PG_MIN_CONNECTIONS /
#   MISSIOND_PG_ACQUIRE_TIMEOUT_SECS optional PostgreSQL pool tuning propagated
#                               to launchd.
#   MISSIOND_BACKGROUND_DB_GRACE_SECS and worker startup limits tune full-os
#                               background DB maintenance so post-deploy MCP
#                               read/query paths keep foreground capacity.
#   MISSION_WS_PORT             daemon HTTP/WebSocket port, default: 9120.
#   MISSIOND_DEPLOY_TIMEOUT     socket readiness timeout, default: 90
#   MISSIOND_DEPLOY_SMOKE_TIMEOUT  MCP smoke timeout, default: 45
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
TIMEOUT="${MISSIOND_DEPLOY_TIMEOUT:-90}"
SMOKE_TIMEOUT="${MISSIOND_DEPLOY_SMOKE_TIMEOUT:-45}"
MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT="${MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT:-auto}"
MISSION_WS_PORT="${MISSION_WS_PORT:-9120}"
RELEASE_KEEP="${MISSIOND_RELEASE_KEEP:-5}"
DEPLOY_LOCK_PATH="${MISSIOND_DEPLOY_LOCK_PATH:-${INSTALL_ROOT}/deploy.lock.d}"
DEPLOY_LOCK_STALE_SECS="${MISSIOND_DEPLOY_LOCK_STALE_SECS:-300}"
BACKUP_RETENTION_DAYS="${MISSIOND_BACKUP_RETENTION_DAYS:-7}"
APPLY_BACKUP_CLEANUP="${MISSIOND_APPLY_BACKUP_CLEANUP:-0}"
PREVIOUS_LAUNCHD_PROJECT_ROOT=""
PREVIOUS_RUNTIME_DIR=""
PREVIOUS_COMPILED_RUNTIME_DIR=""
INITIAL_ACTIVE_RELEASE=""
EXPECTED_ACTIVE_RELEASE=""

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

augment_managed_node_path() {
  local candidates=(
    "${HOME}/.local/share/node-v24.14.0-darwin-arm64/bin"
    "${HOME}/.local/opt/node-v22.13.1-darwin-arm64/bin"
    "${HOME}/.opam/missiond/bin"
    "${HOME}/.opam/default/bin"
    "${HOME}/.local/bin"
    "/opt/homebrew/opt/libpq/bin"
    "/opt/homebrew/bin"
    "/usr/local/opt/libpq/bin"
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

is_transient_repo_root() {
  case "$1" in
    /tmp/*|/private/tmp/*|/var/folders/*) return 0 ;;
    *) return 1 ;;
  esac
}

read_launchd_plist_string_early() {
  local key="$1"
  [ -f "$LAUNCHD_PLIST" ] || return 1
  [ -x /usr/libexec/PlistBuddy ] || return 1
  /usr/libexec/PlistBuddy -c "Print :${key}" "$LAUNCHD_PLIST" 2>/dev/null || return 1
}

active_release_manifest_path_early() {
  local target manifest
  [ -L "$ACTIVE_LINK" ] || return 1
  target="$(readlink "$ACTIVE_LINK")" || return 1
  case "$target" in
    /*) ;;
    *) target="$(cd "$(dirname "$ACTIVE_LINK")" && cd "$(dirname "$target")" && pwd -P)/$(basename "$target")" ;;
  esac
  manifest="$target/release-manifest.json"
  [ -f "$manifest" ] || return 1
  printf '%s\n' "$manifest"
}

read_active_manifest_string_early() {
  local key="$1"
  local manifest
  command -v node >/dev/null 2>&1 || return 1
  manifest="$(active_release_manifest_path_early)" || return 1
  node - "$manifest" "$key" <<'NODE'
const fs = require("node:fs");
const [file, key] = process.argv.slice(2);
const value = JSON.parse(fs.readFileSync(file, "utf8"))[key];
if (typeof value !== "string") process.exit(2);
process.stdout.write(value);
NODE
}

is_stable_owner_root_candidate() {
  local candidate="$1"
  [ -n "$candidate" ] || return 1
  ! is_transient_repo_root "$candidate" || return 1
  [ -d "$candidate/.missiond/v3" ] || return 1
  git -C "$candidate" rev-parse --show-toplevel >/dev/null 2>&1 || return 1
}

select_launchd_project_root() {
  if [ -n "${MISSIOND_LAUNCHD_PROJECT_ROOT:-}" ]; then
    printf '%s\n' "$MISSIOND_LAUNCHD_PROJECT_ROOT"
    return 0
  fi
  if is_transient_repo_root "$REPO_ROOT"; then
    local existing_root
    existing_root="$(read_launchd_plist_string_early "WorkingDirectory" || true)"
    if [ -n "$existing_root" ] && ! is_transient_repo_root "$existing_root" && [ -d "$existing_root/.missiond/v3" ]; then
      printf '[deploy-daemon] launchd: preserving existing stable project root %s; current repo root is transient %s\n' "$existing_root" "$REPO_ROOT" >&2
      printf '%s\n' "$existing_root"
      return 0
    fi
  fi
  printf '%s\n' "$REPO_ROOT"
}

select_deploy_owner_root() {
  if [ -n "${MISSIOND_DEPLOY_OWNER_ROOT:-}" ]; then
    printf '%s\n' "$MISSIOND_DEPLOY_OWNER_ROOT"
    return 0
  fi

  local active_owner_root
  active_owner_root="$(read_active_manifest_string_early "release_owner_root" || true)"
  if is_transient_repo_root "$REPO_ROOT" && is_stable_owner_root_candidate "$active_owner_root"; then
    printf '[deploy-daemon] ownership: preserving active release owner root %s; current repo root is transient %s\n' "$active_owner_root" "$REPO_ROOT" >&2
    printf '%s\n' "$active_owner_root"
    return 0
  fi
  if is_stable_owner_root_candidate "$REPO_ROOT"; then
    printf '%s\n' "$REPO_ROOT"
    return 0
  fi
  if is_stable_owner_root_candidate "${LAUNCHD_PROJECT_ROOT:-}"; then
    printf '[deploy-daemon] ownership: using stable launchd project root %s as deploy owner root\n' "$LAUNCHD_PROJECT_ROOT" >&2
    printf '%s\n' "$LAUNCHD_PROJECT_ROOT"
    return 0
  fi
  printf '%s\n' "$LAUNCHD_PROJECT_ROOT"
}

LAUNCHD_PROJECT_ROOT="$(select_launchd_project_root)"
DEPLOY_OWNER_ROOT="$(select_deploy_owner_root)"
MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT="${MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT:-$DEPLOY_OWNER_ROOT}"
MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER="${MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER:-0}"
MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE="${MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE:-0}"
MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION="${MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION:-0}"
MISSIOND_RELEASE_SOURCE_SNAPSHOT="${MISSIOND_RELEASE_SOURCE_SNAPSHOT:-1}"
REPO_ID="$(basename "$DEPLOY_OWNER_ROOT")"
RUNTIME_DIR="${MISSIOND_RUNTIME_DIR:-${HOME}/.missiond/runtime/${REPO_ID}}"
COMPILED_RUNTIME_DIR="${MISSIOND_COMPILED_RUNTIME_DIR:-${RUNTIME_DIR}/compiled}"
BUILD_ONLY_RUNTIME_TMP=""
CANDIDATE_DIR=""
CANDIDATE_COMPILED_RUNTIME_DIR=""
RELEASE_ID=""
SELF_DEPLOY_LEASE_ROOT="${MISSIOND_SELF_DEPLOY_LEASE_ROOT:-${INSTALL_ROOT}/release-lease.lock}"
SELF_DEPLOY_LEASE_TTL_SECS="${MISSIOND_SELF_DEPLOY_LEASE_TTL_SECS:-1800}"
SELF_DEPLOY_LEASE_HELD=0
SELF_DEPLOY_LEASE_ID=""
export MISSIOND_RUNTIME_DIR="$RUNTIME_DIR"
export MISSIOND_COMPILED_RUNTIME_DIR="$COMPILED_RUNTIME_DIR"

log() { printf '[deploy-daemon] %s\n' "$*" >&2; }
fail() { printf '[deploy-daemon] FAIL: %s\n' "$*" >&2; exit "${2:-1}"; }
cleanup_build_only_runtime_tmp() {
  if [ -n "$BUILD_ONLY_RUNTIME_TMP" ] && [ -d "$BUILD_ONLY_RUNTIME_TMP" ]; then
    rm -rf "$BUILD_ONLY_RUNTIME_TMP"
  fi
}

release_self_deploy_release_lease() {
  if [ "$SELF_DEPLOY_LEASE_HELD" -eq 1 ] && [ -d "$SELF_DEPLOY_LEASE_ROOT" ]; then
    rm -rf "$SELF_DEPLOY_LEASE_ROOT"
    log "release-lease: released $SELF_DEPLOY_LEASE_ID"
  fi
}

cleanup_on_exit() {
  cleanup_build_only_runtime_tmp
  release_self_deploy_release_lease
}
trap cleanup_on_exit EXIT

ensure_codex_app_cli_on_path() {
  if command -v codex >/dev/null 2>&1; then
    return 0
  fi
  local app_cli="/Applications/Codex.app/Contents/Resources/codex"
  local local_bin="${HOME}/.local/bin"
  if [ -x "$app_cli" ]; then
    mkdir -p "$local_bin"
    ln -sfn "$app_cli" "${local_bin}/codex"
    case ":$PATH:" in
      *":$local_bin:"*) ;;
      *) PATH="$local_bin:$PATH" ;;
    esac
    export PATH
    log "bootstrap: linked Codex.app CLI into ${local_bin}/codex"
  fi
}

ensure_codex_app_cli_on_path
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

release_deploy_lock() {
  if [ "$DEPLOY_LOCK_HELD" -ne 1 ]; then
    return 0
  fi
  case "$DEPLOY_LOCK_PATH" in
    ""|"/"|"$HOME"|"$INSTALL_ROOT"|"$RELEASES_DIR")
      log "deploy-lock: refuse to remove unsafe lock path $DEPLOY_LOCK_PATH"
      return 0
      ;;
  esac
  if [ -f "$DEPLOY_LOCK_PATH/pid" ] && [ "$(cat "$DEPLOY_LOCK_PATH/pid" 2>/dev/null || true)" = "$$" ]; then
    rm -rf "$DEPLOY_LOCK_PATH"
    log "deploy-lock: released $DEPLOY_LOCK_PATH"
  else
    log "deploy-lock: owner changed before release; left $DEPLOY_LOCK_PATH untouched"
  fi
  DEPLOY_LOCK_HELD=0
}

try_recover_stale_deploy_lock() {
  [ -d "$DEPLOY_LOCK_PATH" ] || return 1
  local owner_pid lock_mtime now age
  owner_pid="$(cat "$DEPLOY_LOCK_PATH/pid" 2>/dev/null || true)"
  case "$owner_pid" in
    ""|*[!0-9]*)
      lock_mtime="$(stat -f %m "$DEPLOY_LOCK_PATH" 2>/dev/null || stat -c %Y "$DEPLOY_LOCK_PATH" 2>/dev/null || echo 0)"
      now="$(date +%s)"
      age=$(( now - lock_mtime ))
      if [ "$age" -lt "$DEPLOY_LOCK_STALE_SECS" ]; then
        log "deploy-lock: metadata missing but lock is fresh age=${age}s stale_after=${DEPLOY_LOCK_STALE_SECS}s"
        return 1
      fi
      log "deploy-lock: removing stale metadata-less lock $DEPLOY_LOCK_PATH age=${age}s"
      ;;
    *)
      if kill -0 "$owner_pid" >/dev/null 2>&1; then
        return 1
      fi
      log "deploy-lock: removing stale lock $DEPLOY_LOCK_PATH owned by exited pid=$owner_pid"
      ;;
  esac
  rm -rf "$DEPLOY_LOCK_PATH"
  return 0
}

acquire_deploy_lock() {
  mkdir -p "$INSTALL_ROOT"
  local lock_created=0
  if mkdir "$DEPLOY_LOCK_PATH" 2>/dev/null; then
    lock_created=1
  else
    try_recover_stale_deploy_lock || true
  fi
  if [ "$lock_created" -ne 1 ] && ! mkdir "$DEPLOY_LOCK_PATH" 2>/dev/null; then
    log "deploy-lock: busy $DEPLOY_LOCK_PATH"
    if [ -f "$DEPLOY_LOCK_PATH/owner" ]; then
      sed 's/^/[deploy-lock-owner] /' "$DEPLOY_LOCK_PATH/owner" >&2 || true
    fi
    return 1
  fi
  DEPLOY_LOCK_HELD=1
  trap release_deploy_lock EXIT
  {
    printf 'schema=missiond.deploy-lock.v1\n'
    printf 'pid=%s\n' "$$"
    printf 'repo_root=%s\n' "$REPO_ROOT"
    printf 'install_root=%s\n' "$INSTALL_ROOT"
    printf 'active_link=%s\n' "$ACTIVE_LINK"
    printf 'profile=%s\n' "$PROFILE"
    printf 'started_at=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  } > "$DEPLOY_LOCK_PATH/owner"
  printf '%s\n' "$$" > "$DEPLOY_LOCK_PATH/pid"
  log "deploy-lock: acquired $DEPLOY_LOCK_PATH"
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
  mkdir -p "$INSTALL_ROOT"
  MISSIOND_MCP_BIN_PATH_VALUE="$MCP_BIN_PATH" \
    MISSIOND_SOCKET_PATH_VALUE="$SOCK_PATH" \
    node - "$config_path" <<'NODE'
const fs = require("node:fs");
const path = process.argv[2];
const missiond = {
  command: process.env.MISSIOND_MCP_BIN_PATH_VALUE,
  args: [],
  env: { MISSIOND_SOCKET_PATH: process.env.MISSIOND_SOCKET_PATH_VALUE },
};

let config = {};
if (fs.existsSync(path)) {
  const text = fs.readFileSync(path, "utf8").trim();
  if (text.length > 0) {
    config = JSON.parse(text);
  }
}
if (!config || Array.isArray(config) || typeof config !== "object") {
  throw new Error("MCP config root must be a JSON object");
}
if (config.mcpServers == null) {
  config.mcpServers = {};
}
if (
  !config.mcpServers ||
  Array.isArray(config.mcpServers) ||
  typeof config.mcpServers !== "object"
) {
  throw new Error("mcpServers must be a JSON object");
}
const previous = JSON.stringify(config.mcpServers.missiond ?? null);
const next = JSON.stringify(missiond);
if (previous !== next) {
  config.mcpServers.missiond = missiond;
  const tmp = `${path}.tmp-${process.pid}`;
  fs.writeFileSync(tmp, `${JSON.stringify(config, null, 2)}\n`, { mode: 0o600 });
  fs.renameSync(tmp, path);
}
NODE
  chmod 600 "$config_path"
  validate_default_mcp_config "$config_path"
  log "mcp-config: ensured MissionD MCP server in $config_path"
}

validate_default_mcp_config() {
  local config_path="$1"
  node -e 'const fs=require("node:fs"); const p=process.argv[1]; const data=fs.readFileSync(p,"utf8"); JSON.parse(data); if ((data.match(/"mcpServers"/g)||[]).length !== 1) throw new Error("duplicate mcpServers JSON object");' "$config_path" ||
    fail "mcp-config JSON validation failed: $config_path" 1
  log "mcp-config: JSON validation OK $config_path"
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
  local start elapsed resp
  start="$(date +%s)"
  while true; do
    if [ -S "$SOCK_PATH" ]; then
      resp="$(run_mcp_initialize_smoke "$MCP_BIN_PATH" 2>&1 | tail -3 || true)"
      if echo "$resp" | grep -q '"protocolVersion"'; then
        elapsed=$(( $(date +%s) - start ))
        log "ready: IPC initialize succeeded after ${elapsed}s"
        return 0
      fi
      echo "$resp" | sed 's/^/[wait-smoke] /' >&2
      log "wait: socket exists but IPC initialize is not ready yet"
    else
      log "wait: socket file not present yet"
    fi
    elapsed=$(( $(date +%s) - start ))
    if [ "$elapsed" -ge "$TIMEOUT" ]; then
      elapsed=$(( $(date +%s) - start ))
      log "wait: socket/IPC readiness timed out after ${elapsed}s"
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

plist_set_env_from_current_env() {
  local plist="$1"
  local key="$2"
  local value="${!key:-}"
  if [ -n "$value" ]; then
    plist_set_or_add_env_string "$plist" "$key" "$value"
  fi
}

plist_delete_env_if_present() {
  local plist="$1"
  local key="$2"
  local buddy="/usr/libexec/PlistBuddy"
  if "$buddy" -c "Print :EnvironmentVariables:${key}" "$plist" >/dev/null 2>&1; then
    "$buddy" -c "Delete :EnvironmentVariables:${key}" "$plist" >/dev/null
  fi
}

ensure_provider_box_internal_token() {
  if [ -n "${MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN:-}" ] || [ -n "${MISSIOND_AGY_INTERNAL_TOKEN:-}" ]; then
    return 0
  fi
  local existing_token=""
  existing_token="$(plist_read_string "$LAUNCHD_PLIST" "EnvironmentVariables:MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN" || true)"
  if [ -n "$existing_token" ]; then
    MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN="$existing_token"
    export MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN
    log "launchd: preserved existing MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN for provider-box internal HTTP auth"
    return 0
  fi
  existing_token="$(plist_read_string "$LAUNCHD_PLIST" "EnvironmentVariables:MISSIOND_AGY_INTERNAL_TOKEN" || true)"
  if [ -n "$existing_token" ]; then
    MISSIOND_AGY_INTERNAL_TOKEN="$existing_token"
    export MISSIOND_AGY_INTERNAL_TOKEN
    log "launchd: preserved existing MISSIOND_AGY_INTERNAL_TOKEN for provider-box internal HTTP auth"
    return 0
  fi
  command -v uuidgen >/dev/null 2>&1 ||
    fail "uuidgen not on PATH; cannot generate MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN" 1
  MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN="$(uuidgen | tr '[:upper:]' '[:lower:]')"
  export MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN
  log "launchd: generated MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN for provider-box internal HTTP auth"
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
  ensure_provider_box_internal_token

  plist_set_or_add_string "$LAUNCHD_PLIST" "WorkingDirectory" "$LAUNCHD_PROJECT_ROOT"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_PROJECT_ROOT" "$LAUNCHD_PROJECT_ROOT"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_ORCHESTRATOR_ROOT" "$LAUNCHD_PROJECT_ROOT"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_SOCKET_PATH" "$SOCK_PATH"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_RUNTIME_DIR" "$RUNTIME_DIR"
  plist_set_or_add_env_string "$LAUNCHD_PLIST" "MISSIOND_COMPILED_RUNTIME_DIR" "$COMPILED_RUNTIME_DIR"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_PG_MAX_CONNECTIONS"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_PG_MIN_CONNECTIONS"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_PG_ACQUIRE_TIMEOUT_SECS"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_BACKGROUND_DB_GRACE_SECS"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_MESSAGE_LABELER_STARTUP_LIMIT"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_TAGGER_STARTUP_LIMIT"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_SLOT_AUTO_HEAL"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_SLOT_AUTO_HEAL_TIMEOUT_SECS"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FULL_OS_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_WORKFLOW_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_MEMORY_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_SKILL_STORE_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_ROUTER_EXPERIMENTS_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_CODEX_REPLAY_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_SELF_EVOLUTION_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_CONVERSATIONS_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_INFRA_OS_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_FEATURE_BOARD_ADVANCED_ENABLE"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_INTERACTION_SERVICE_TOKEN"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_INTERACTION_AUTH_USERINFO_URL"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_INTERACTION_AUTH_TIMEOUT_MS"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_XJPCODE_WORKER_URL"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_AGY_INTERNAL_TOKEN"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_PROVIDER_BOX_PROXY_BASE_URL"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_AGY_PROVIDER_BOX_BASE_URL"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_AUTHOR_TEXT_ONLY_PROVIDER"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_AUTHOR_TEXT_ONLY_MODEL"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_AUTHOR_TEXT_ONLY_SLOT_ID"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_DIRECT_ANSWER_PROVIDER"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_DIRECT_ANSWER_MODEL"
  plist_set_env_from_current_env "$LAUNCHD_PLIST" "MISSIOND_JARVIS_DIRECT_ANSWER_TIMEOUT_SECS"
  plist_delete_env_if_present "$LAUNCHD_PLIST" "MISSIOND_XJPCODE_TEXT_ONLY_URL"
  plist_delete_env_if_present "$LAUNCHD_PLIST" "MISSIOND_XJPCODE_TEXT_ONLY_ENDPOINT"
  plist_delete_env_if_present "$LAUNCHD_PLIST" "MISSIOND_XJPCODE_BASE_URL"
  plutil -lint "$LAUNCHD_PLIST" >/dev/null
  log "launchd: runtime root $LAUNCHD_PROJECT_ROOT written to $LAUNCHD_PLIST"
  log "launchd: artifact runtime dir $RUNTIME_DIR written to $LAUNCHD_PLIST"
}

plist_read_string() {
  local plist="$1"
  local key="$2"
  local buddy="/usr/libexec/PlistBuddy"
  [ -f "$plist" ] || return 1
  [ -x "$buddy" ] || return 1
  "$buddy" -c "Print :${key}" "$plist" 2>/dev/null || return 1
}

json_string_field() {
  local file="$1"
  local key="$2"
  node - "$file" "$key" <<'NODE'
const fs = require("node:fs");
const [file, key] = process.argv.slice(2);
const value = JSON.parse(fs.readFileSync(file, "utf8"))[key];
if (typeof value !== "string") process.exit(2);
process.stdout.write(value);
NODE
}

assert_active_project_root_can_mutate() {
  local phase="$1"
  local active manifest active_owner_root active_project_root manifest_launchd_project_root launchd_project_root expected_root
  expected_root="${MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT:-$DEPLOY_OWNER_ROOT}"
  if truthy_env_value "$MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER"; then
    log "ownership: project-root takeover explicitly allowed phase=$phase expected_active_root=$expected_root deploy_owner_root=$DEPLOY_OWNER_ROOT"
    return 0
  fi
  active="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null || true)"
  if [ -z "$active" ]; then
    log "ownership: no active release link phase=$phase; project-root guard allows initial deploy"
    return 0
  fi
  manifest="$active/release-manifest.json"
  if [ ! -f "$manifest" ]; then
    log "ownership: active release has no manifest phase=$phase active=$active; project-root guard allows legacy migration"
    return 0
  fi
  active_owner_root="$(json_string_field "$manifest" "release_owner_root" 2>/dev/null || true)"
  active_project_root="$(json_string_field "$manifest" "launchd_project_root" 2>/dev/null || true)"
  manifest_launchd_project_root="$active_project_root"
  [ -n "$active_owner_root" ] || active_owner_root="$active_project_root"
  if [ -n "$active_owner_root" ] && [ "$active_owner_root" != "$expected_root" ]; then
    log "ownership: active project-root mismatch phase=$phase expected=$expected_root active_owner_root=$active_owner_root active_runtime_root=$active_project_root active=$active"
    log "ownership: set MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER=1 only for an intentional cross-root active release takeover"
    return 1
  fi
  launchd_project_root="$(plist_read_string "$LAUNCHD_PLIST" "WorkingDirectory" || true)"
  if [ -n "$launchd_project_root" ]; then
    if [ -n "$manifest_launchd_project_root" ]; then
      if [ "$launchd_project_root" != "$manifest_launchd_project_root" ]; then
        log "ownership: launchd runtime-root mismatch phase=$phase manifest_runtime_root=$manifest_launchd_project_root launchd_project_root=$launchd_project_root"
        log "ownership: set MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER=1 only for an intentional launchd/runtime takeover"
        return 1
      fi
    elif [ "$launchd_project_root" != "$expected_root" ]; then
      log "ownership: launchd project-root mismatch phase=$phase expected=$expected_root launchd_project_root=$launchd_project_root"
      log "ownership: set MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER=1 only for an intentional cross-root launchd takeover"
      return 1
    fi
  fi
  log "ownership: project-root mutation guard verified phase=$phase owner_root=$expected_root"
}

assert_active_release_owned() {
  assert_active_project_root_can_mutate "$1"
}

capture_launchd_runtime_state() {
  PREVIOUS_LAUNCHD_PROJECT_ROOT="$(plist_read_string "$LAUNCHD_PLIST" "WorkingDirectory" || true)"
  PREVIOUS_RUNTIME_DIR="$(plist_read_string "$LAUNCHD_PLIST" "EnvironmentVariables:MISSIOND_RUNTIME_DIR" || true)"
  PREVIOUS_COMPILED_RUNTIME_DIR="$(plist_read_string "$LAUNCHD_PLIST" "EnvironmentVariables:MISSIOND_COMPILED_RUNTIME_DIR" || true)"
  [ -n "$PREVIOUS_LAUNCHD_PROJECT_ROOT" ] || PREVIOUS_LAUNCHD_PROJECT_ROOT="$LAUNCHD_PROJECT_ROOT"
  [ -n "$PREVIOUS_RUNTIME_DIR" ] || PREVIOUS_RUNTIME_DIR="$RUNTIME_DIR"
  [ -n "$PREVIOUS_COMPILED_RUNTIME_DIR" ] || PREVIOUS_COMPILED_RUNTIME_DIR="$COMPILED_RUNTIME_DIR"
  log "launchd: captured previous runtime root $PREVIOUS_LAUNCHD_PROJECT_ROOT"
  log "launchd: captured previous artifact runtime dir $PREVIOUS_RUNTIME_DIR"
}

restart_daemon_supervisor_for_runtime() {
  local project_root="$1"
  local runtime_dir="$2"
  local compiled_runtime_dir="$3"
  LAUNCHD_PROJECT_ROOT="$project_root"
  RUNTIME_DIR="$runtime_dir"
  COMPILED_RUNTIME_DIR="$compiled_runtime_dir"
  export MISSIOND_RUNTIME_DIR="$RUNTIME_DIR"
  export MISSIOND_COMPILED_RUNTIME_DIR="$COMPILED_RUNTIME_DIR"
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

restart_daemon_supervisor() {
  restart_daemon_supervisor_for_runtime "$LAUNCHD_PROJECT_ROOT" "$RUNTIME_DIR" "$COMPILED_RUNTIME_DIR"
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

pre_switch_mcp_smoke() {
  local mcp="$1"
  local current_resp resp
  if [ "$DO_SMOKE" -eq 0 ]; then
    log "pre-switch smoke: skipped by --no-smoke"
    return 0
  fi
  if [ ! -S "$SOCK_PATH" ]; then
    log "pre-switch smoke: current socket missing; defer candidate MCP initialize until post-switch"
    return 0
  fi
  current_resp="$(run_mcp_initialize_smoke "$MCP_BIN_PATH" 2>&1 | tail -3 || true)"
  if ! echo "$current_resp" | grep -q '"protocolVersion"'; then
    log "pre-switch smoke: current IPC is not healthy; defer candidate MCP initialize until post-switch"
    echo "$current_resp" | sed 's/^/[pre-smoke-current] /' >&2
    return 0
  fi
  resp="$(run_mcp_initialize_smoke "$mcp" 2>&1 | tail -3 || true)"
  if echo "$resp" | grep -q '"protocolVersion"'; then
    log "pre-switch smoke: candidate MCP initialize OK against current daemon"
    return 0
  fi
  echo "$resp" | sed 's/^/[pre-smoke] /' >&2
  return 1
}

truthy_env_value() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    1|true|yes|on) return 0 ;;
    *) return 1 ;;
  esac
}

acquire_self_deploy_release_lease() {
  mkdir -p "$INSTALL_ROOT"
  SELF_DEPLOY_LEASE_ID="missiond-daemon:${RELEASE_ID}:$$"
  if ! mkdir "$SELF_DEPLOY_LEASE_ROOT" 2>/dev/null; then
    log "release-lease: conflict lock=$SELF_DEPLOY_LEASE_ROOT"
    if [ -f "$SELF_DEPLOY_LEASE_ROOT/release-lease.json" ]; then
      sed 's/^/[release-lease-conflict] /' "$SELF_DEPLOY_LEASE_ROOT/release-lease.json" >&2 || true
    fi
    return 1
  fi
  SELF_DEPLOY_LEASE_HELD=1
  SELF_DEPLOY_LEASE_ROOT_VALUE="$SELF_DEPLOY_LEASE_ROOT" \
  SELF_DEPLOY_LEASE_ID_VALUE="$SELF_DEPLOY_LEASE_ID" \
  SELF_DEPLOY_LEASE_TTL_SECS_VALUE="$SELF_DEPLOY_LEASE_TTL_SECS" \
  SELF_DEPLOY_RELEASE_ID_VALUE="$RELEASE_ID" \
  SELF_DEPLOY_SERVICE_VALUE="missiond-daemon" \
  SELF_DEPLOY_RUNTIME_TARGET_VALUE="local-launchd" \
  SELF_DEPLOY_OWNER_VALUE="scripts/deploy-daemon.sh:$$" \
  SELF_DEPLOY_OWNER_ROOT_VALUE="$DEPLOY_OWNER_ROOT" \
  SELF_DEPLOY_EXPECTED_ACTIVE_ROOT_VALUE="${MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT:-$DEPLOY_OWNER_ROOT}" \
  SELF_DEPLOY_EXPECTED_ACTIVE_RELEASE_VALUE="${EXPECTED_ACTIVE_RELEASE:-}" \
  SELF_DEPLOY_INITIAL_ACTIVE_VALUE="${INITIAL_ACTIVE_RELEASE:-}" \
  SELF_DEPLOY_ACTIVE_LINK_VALUE="$ACTIVE_LINK" \
  SELF_DEPLOY_GIT_SHA_VALUE="$GIT_FULL_SHA" \
  node <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const root = process.env.SELF_DEPLOY_LEASE_ROOT_VALUE;
const now = new Date();
const ttlSecs = Number(process.env.SELF_DEPLOY_LEASE_TTL_SECS_VALUE || 1800);
const lease = {
  schema: 'missiond.release-lease.v1',
  id: process.env.SELF_DEPLOY_LEASE_ID_VALUE,
  service: process.env.SELF_DEPLOY_SERVICE_VALUE,
  runtime_target: process.env.SELF_DEPLOY_RUNTIME_TARGET_VALUE,
  release_id: process.env.SELF_DEPLOY_RELEASE_ID_VALUE,
  owner: process.env.SELF_DEPLOY_OWNER_VALUE,
  expected_active_root: process.env.SELF_DEPLOY_EXPECTED_ACTIVE_ROOT_VALUE || null,
  expected_active_release: process.env.SELF_DEPLOY_EXPECTED_ACTIVE_RELEASE_VALUE || null,
  initial_active_release: process.env.SELF_DEPLOY_INITIAL_ACTIVE_VALUE || null,
  expected_running_digest: process.env.SELF_DEPLOY_GIT_SHA_VALUE || null,
  active_link: process.env.SELF_DEPLOY_ACTIVE_LINK_VALUE || null,
  owner_root: process.env.SELF_DEPLOY_OWNER_ROOT_VALUE || null,
  conflict_policy: 'fail-closed-local-lease-project-root-active-release-generation-and-commit-ancestry',
  created_at: now.toISOString(),
  expires_at: new Date(now.getTime() + ttlSecs * 1000).toISOString(),
};
fs.writeFileSync(path.join(root, 'release-lease.json'), `${JSON.stringify(lease, null, 2)}\n`, { mode: 0o600 });
NODE
  chmod 700 "$SELF_DEPLOY_LEASE_ROOT"
  log "release-lease: acquired $SELF_DEPLOY_LEASE_ID lock=$SELF_DEPLOY_LEASE_ROOT"
}

capture_expected_active_release() {
  INITIAL_ACTIVE_RELEASE="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null || true)"
  EXPECTED_ACTIVE_RELEASE="${MISSIOND_DEPLOY_EXPECTED_ACTIVE_RELEASE:-$INITIAL_ACTIVE_RELEASE}"
  if [ -n "$EXPECTED_ACTIVE_RELEASE" ]; then
    log "ownership: expected active release $EXPECTED_ACTIVE_RELEASE"
  else
    log "ownership: expected no active release"
  fi
}

assert_active_release_matches_expected() {
  local phase="$1"
  local current
  if truthy_env_value "$MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE"; then
    log "ownership: active release race override phase=$phase expected=${EXPECTED_ACTIVE_RELEASE:-none}"
    return 0
  fi
  current="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null || true)"
  if [ -z "$EXPECTED_ACTIVE_RELEASE" ]; then
    if [ -n "$current" ]; then
      log "ownership: active release appeared phase=$phase expected=none current=$current"
      log "ownership: set MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE=1 only for an intentional concurrent active switch"
      return 1
    fi
  elif [ "$current" != "$EXPECTED_ACTIVE_RELEASE" ]; then
    log "ownership: active release changed phase=$phase expected=$EXPECTED_ACTIVE_RELEASE current=${current:-none}"
    log "ownership: set MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE=1 only for an intentional concurrent active switch"
    return 1
  fi
  log "ownership: active release generation guard verified phase=$phase active=${current:-none}"
}

active_release_git_sha() {
  local active manifest sha
  active="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null || true)"
  [ -n "$active" ] || return 1
  manifest="$active/release-manifest.json"
  [ -f "$manifest" ] || return 1
  sha="$(json_string_field "$manifest" "git_full_sha" 2>/dev/null || true)"
  [ -n "$sha" ] || sha="$(json_string_field "$manifest" "git_sha" 2>/dev/null || true)"
  [ -n "$sha" ] || return 1
  printf '%s\n' "$sha"
}

git_commit_resolves() {
  local sha="$1"
  [ -n "$sha" ] || return 1
  [ "$sha" != "unknown" ] || return 1
  git rev-parse --verify "${sha}^{commit}" >/dev/null 2>&1
}

assert_candidate_commit_not_behind_active() {
  local phase="$1"
  local candidate_sha="$2"
  local active_sha
  if truthy_env_value "$MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION"; then
    log "ownership: commit regression override phase=$phase candidate=$candidate_sha"
    return 0
  fi
  active_sha="$(active_release_git_sha || true)"
  if [ -z "$active_sha" ] || [ "$active_sha" = "unknown" ]; then
    log "ownership: active commit unavailable phase=$phase; commit ancestry guard allows legacy/initial deploy"
    return 0
  fi
  if ! git_commit_resolves "$candidate_sha"; then
    log "ownership: candidate commit cannot be resolved phase=$phase candidate=$candidate_sha"
    log "ownership: set MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION=1 only for an intentional rollback or branch-divergence deploy"
    return 1
  fi
  if ! git_commit_resolves "$active_sha"; then
    log "ownership: active commit cannot be resolved phase=$phase active_commit=$active_sha"
    log "ownership: set MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION=1 only when active provenance cannot be verified but rollback/divergence is intentional"
    return 1
  fi
  if git merge-base --is-ancestor "$active_sha" "$candidate_sha" >/dev/null 2>&1; then
    log "ownership: active commit ancestry guard verified phase=$phase active_commit=$active_sha candidate=$candidate_sha"
    return 0
  fi
  log "ownership: candidate commit is not a descendant of active release commit phase=$phase active_commit=$active_sha candidate=$candidate_sha"
  log "ownership: set MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION=1 only for an intentional rollback or branch-divergence deploy"
  return 1
}

should_ensure_jarvis_slot() {
  case "$(printf '%s' "$MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT" | tr '[:upper:]' '[:lower:]')" in
    0|false|no|off) return 1 ;;
    1|true|yes|on) return 0 ;;
    auto|"")
      if truthy_env_value "${MISSIOND_JARVIS_SLOT_AUTO_HEAL:-}"; then
        return 0
      fi
      local plist_auto_heal
      plist_auto_heal="$(plist_read_string "$LAUNCHD_PLIST" "EnvironmentVariables:MISSIOND_JARVIS_SLOT_AUTO_HEAL" || true)"
      truthy_env_value "$plist_auto_heal"
      return $?
      ;;
    *) fail "unsupported MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT=$MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT" 1 ;;
  esac
}

post_switch_jarvis_slot_ensure() {
  if ! should_ensure_jarvis_slot; then
    log "jarvis-slot: ensure skipped (MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT=$MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT)"
    return 0
  fi
  command -v curl >/dev/null 2>&1 || fail "curl not on PATH; cannot run Jarvis slot ensure smoke" 6
  local url body status elapsed start ensure_mode
  ensure_mode="$(printf '%s' "$MISSIOND_DEPLOY_ENSURE_JARVIS_SLOT" | tr '[:upper:]' '[:lower:]')"
  url="http://127.0.0.1:${MISSION_WS_PORT}/internal/jarvis/slot/ensure"
  log "jarvis-slot: ensure $url"
  start="$(date +%s)"
  while true; do
    body="$(curl -m 20 -sS -X POST -w $'\n%{http_code}' "$url" 2>&1 || true)"
    status="$(printf '%s\n' "$body" | tail -1)"
    body="$(printf '%s\n' "$body" | sed '$d')"
    if [ "$status" = "200" ] && printf '%s' "$body" | grep -q '"overall":"ready"'; then
      log "jarvis-slot: default slot ready"
      return 0
    fi
    elapsed=$(( $(date +%s) - start ))
    [ "$elapsed" -lt "$SMOKE_TIMEOUT" ] || break
    log "jarvis-slot: not ready yet status=${status:-unknown}; retrying..."
    sleep 2
  done
  log "jarvis-slot: ensure failed -- response below"
  printf '%s\n' "$body" | sed 's/^/[jarvis-slot] /' >&2
  if { [ "$ensure_mode" = "auto" ] || [ -z "$ensure_mode" ]; } &&
    [ "$status" = "409" ] &&
    printf '%s' "$body" | grep -q '"overall":"busy"'; then
    log "jarvis-slot: default slot is busy; continuing because ensure mode is auto"
    return 0
  fi
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
  if [ -n "$PREVIOUS_LAUNCHD_PROJECT_ROOT" ] && [ -n "$PREVIOUS_RUNTIME_DIR" ] && [ -n "$PREVIOUS_COMPILED_RUNTIME_DIR" ]; then
    log "rollback: restoring launchd runtime root $PREVIOUS_LAUNCHD_PROJECT_ROOT"
    restart_daemon_supervisor_for_runtime "$PREVIOUS_LAUNCHD_PROJECT_ROOT" "$PREVIOUS_RUNTIME_DIR" "$PREVIOUS_COMPILED_RUNTIME_DIR" >/dev/null 2>&1 || true
  else
    restart_daemon_supervisor >/dev/null 2>&1 || true
  fi
  return 0
}

rollback_with_smoke() {
  local previous="$1"
  local start resp
  start="$(date +%s)"
  if ! rollback_to_previous "$previous"; then
    record_timing "rollback-switch" "$start"
    log "rollback-smoke: skipped because rollback switch failed"
    return 1
  fi
  record_timing "rollback-switch" "$start"
  if [ "$DO_SMOKE" -eq 0 ]; then
    log "rollback-smoke: skipped by --no-smoke"
    return 0
  fi
  start="$(date +%s)"
  resp="$(run_mcp_initialize_smoke "$MCP_BIN_PATH" 2>&1 | tail -3 || true)"
  if echo "$resp" | grep -q '"protocolVersion"'; then
    record_timing "rollback-smoke" "$start"
    log "rollback-smoke: active MCP responded OK after rollback"
    return 0
  fi
  record_timing "rollback-smoke" "$start"
  log "rollback-smoke: active MCP failed after rollback -- stderr tail below"
  echo "$resp" | sed 's/^/[rollback-smoke] /' >&2
  return 1
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
  deploymentPolicy: 'compiled-deployment-policy.json',
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

create_release_source_snapshot() {
  local dir="$1"
  if ! truthy_env_value "$MISSIOND_RELEASE_SOURCE_SNAPSHOT"; then
    printf '%s\n' "$REPO_ROOT"
    return 0
  fi
  command -v tar >/dev/null 2>&1 || fail "tar not on PATH; cannot create release source snapshot" 1
  if ! truthy_env_value "${MISSIOND_RELEASE_ALLOW_DIRTY_SOURCE:-0}"; then
    local dirty
    dirty="$(git status --porcelain -- \
      .missiond/v3 \
      scripts/compile-v3-runtime.mjs \
      scripts/generated \
      crates/missiond-daemon/src/context/v3_contracts/generated.rs \
      crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs \
      2>/dev/null || true)"
    if [ -n "$dirty" ]; then
      printf '%s\n' "$dirty" | sed 's/^/[release-source-dirty] /' >&2
      fail "release source snapshot requires clean V3/runtime projection inputs; commit/stash them or set MISSIOND_RELEASE_ALLOW_DIRTY_SOURCE=1 for a non-reproducible dev deploy" 1
    fi
    mkdir -p "$dir"
    git archive --format=tar HEAD | tar -xf - -C "$dir"
  else
    command -v rsync >/dev/null 2>&1 || fail "rsync not on PATH; cannot create dirty release source snapshot" 1
    rsync -a --delete \
      --exclude '.git' \
      --exclude 'target' \
      --exclude 'node_modules' \
      --exclude '.missiond/v3/runtime' \
      "$REPO_ROOT/" "$dir/"
  fi
  [ -d "$dir/.missiond/v3" ] || fail "release source snapshot missing .missiond/v3: $dir" 1
  log "release-source: snapshot $dir"
  printf '%s\n' "$dir"
}

write_self_deploy_closure_files() {
  local dir="$1"
  local verdict="$2"
  local smoke_status="$3"
  local next_action="$4"
  SELF_DEPLOY_RELEASE_DIR="$dir" \
  SELF_DEPLOY_VERDICT="$verdict" \
  SELF_DEPLOY_SMOKE_STATUS="$smoke_status" \
  SELF_DEPLOY_NEXT_ACTION="$next_action" \
  SELF_DEPLOY_ACTIVE_LINK="$ACTIVE_LINK" \
  SELF_DEPLOY_LEASE_ID="${SELF_DEPLOY_LEASE_ID:-}" \
  SELF_DEPLOY_LEASE_ROOT="${SELF_DEPLOY_LEASE_ROOT:-}" \
  SELF_DEPLOY_PREVIOUS_ACTIVE="${PREVIOUS_ACTIVE:-}" \
  SELF_DEPLOY_INITIAL_ACTIVE="${INITIAL_ACTIVE_RELEASE:-}" \
  SELF_DEPLOY_EXPECTED_ACTIVE_RELEASE="${EXPECTED_ACTIVE_RELEASE:-}" \
  SELF_DEPLOY_EXPECTED_ACTIVE_ROOT="${MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT:-$LAUNCHD_PROJECT_ROOT}" \
  node <<'NODE'
const fs = require('node:fs');
const path = require('node:path');

const dir = process.env.SELF_DEPLOY_RELEASE_DIR;
const manifestPath = path.join(dir, 'release-manifest.json');
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const now = new Date().toISOString();
const verdict = process.env.SELF_DEPLOY_VERDICT || 'blocked';
const smoke = process.env.SELF_DEPLOY_SMOKE_STATUS || 'unknown';
const blockers = [];
if (verdict !== 'success') blockers.push(`self_deploy_${verdict}`);
if (smoke !== 'passed') blockers.push(`smoke_${smoke}`);

const evidence = {
  schema: 'missiond.release-evidence.v1',
  authority: 'missiond-self-deploy',
  deployment_intent: {
    project: 'missiond',
    service: 'missiond-daemon',
    runtime_target: 'local-launchd',
    change_class: 'missiond-self-update',
    desired_commit: manifest.git_full_sha || manifest.git_sha,
    deployment_policy_hash: manifest.typed_lisp_runtime?.projections?.deploymentPolicy?.source_hash ?? null,
  },
  release_candidate: {
    release_id: manifest.release_id,
    git_sha: manifest.git_sha,
    daemon_sha256: manifest.daemon_sha256,
    mcp_sha256: manifest.mcp_sha256,
    source_snapshot: manifest.launchd_project_root,
    compiled_runtime_dir: manifest.compiled_runtime_dir,
    compiled_abi_hash: manifest.typed_lisp_runtime?.projections?.v3?.source_hash ?? null,
  },
  release_lease: {
    id: process.env.SELF_DEPLOY_LEASE_ID || null,
    service: 'missiond-daemon',
    runtime_target: 'local-launchd',
    owner: 'scripts/deploy-daemon.sh',
    lock_path: process.env.SELF_DEPLOY_LEASE_ROOT || null,
    expected_active_root: process.env.SELF_DEPLOY_EXPECTED_ACTIVE_ROOT || null,
    expected_active_release: process.env.SELF_DEPLOY_EXPECTED_ACTIVE_RELEASE || null,
    expected_active_commit: manifest.active_git_sha || null,
    initial_active_release: process.env.SELF_DEPLOY_INITIAL_ACTIVE || null,
    active_link: process.env.SELF_DEPLOY_ACTIVE_LINK || null,
    previous_active: process.env.SELF_DEPLOY_PREVIOUS_ACTIVE || null,
    conflict_policy: 'fail-closed-project-root-active-release-generation-and-commit-ancestry-guard',
  },
  runtime_observation: {
    active_release_dir: dir,
    git_full_sha: manifest.git_full_sha || null,
    release_owner_root: manifest.release_owner_root ?? manifest.launchd_project_root,
    launchd_project_root: manifest.launchd_project_root,
    runtime_dir: manifest.runtime_dir,
    compiled_runtime_dir: manifest.compiled_runtime_dir,
    binary_marker: manifest.daemon_sha256,
    mcp_marker: manifest.mcp_sha256,
  },
  smoke: { status: smoke },
  secret_availability: { status: 'not_required' },
  rollback_artifact_refs: process.env.SELF_DEPLOY_PREVIOUS_ACTIVE ? [process.env.SELF_DEPLOY_PREVIOUS_ACTIVE] : [],
  created_at: now,
};

const closure = {
  schema: 'missiond.closure-verdict.v1',
  authority: 'missiond-self-deploy',
  release_id: manifest.release_id,
  project: 'missiond',
  service: 'missiond-daemon',
  verdict,
  typed_diagnostics: blockers,
  next_action: process.env.SELF_DEPLOY_NEXT_ACTION || null,
  confidence: verdict === 'success' ? 'high' : 'partial',
  evidence_ref: 'release-evidence.json',
  created_at: now,
};

fs.writeFileSync(path.join(dir, 'release-evidence.json'), `${JSON.stringify(evidence, null, 2)}\n`);
fs.writeFileSync(path.join(dir, 'closure-verdict.json'), `${JSON.stringify(closure, null, 2)}\n`);
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

cleanup_repo_runtime_cache() {
  if [ "${MISSIOND_CLEAN_REPO_RUNTIME_CACHE:-1}" != "1" ]; then
    log "runtime-cache: repo runtime cleanup disabled by MISSIOND_CLEAN_REPO_RUNTIME_CACHE"
    return 0
  fi
  local repo_runtime="$REPO_ROOT/.missiond/v3/runtime"
  if [ "$RUNTIME_DIR" = "$repo_runtime" ] || [ "$COMPILED_RUNTIME_DIR" = "$repo_runtime/compiled" ]; then
    log "runtime-cache: external runtime dir not configured; keep repo runtime cache"
    return 0
  fi
  if [ ! -f "$COMPILED_RUNTIME_DIR/compiled-runtime-config.json" ]; then
    log "runtime-cache: skip cleanup; compiled runtime config missing at $COMPILED_RUNTIME_DIR"
    return 0
  fi
  [ -d "$repo_runtime" ] || return 0

  local item
  local cleaned=0
  for item in \
    "$repo_runtime/compiled" \
    "$repo_runtime/executions" \
    "$repo_runtime/plans" \
    "$repo_runtime/context-gather" \
    "$repo_runtime/lisp-code-sync" \
	    "$repo_runtime/nightly-evolution" \
	    "$repo_runtime/jarvis-smoke" \
	    "$repo_runtime/master-control" \
	    "$repo_runtime/master-control-checkpoint.lisp" \
	    "$repo_runtime/genome" \
	    "$repo_runtime/capability-usage-review.json"; do
    if [ -e "$item" ]; then
      rm -rf "$item"
      cleaned=$((cleaned + 1))
      log "runtime-cache: removed repo cache $item"
    fi
  done
  log "runtime-cache: cleanup complete removed=$cleaned external_runtime=$RUNTIME_DIR"
}

GIT_FULL_SHA="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
GIT_SHA="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"

mkdir -p "$INSTALL_ROOT" "$RELEASES_DIR" "$(dirname "$SOCK_PATH")"
capture_expected_active_release

if [ "$DO_DEPLOY" -eq 1 ] || { [ "$CLEANUP_ONLY" -eq 1 ] && [ "$APPLY_CLEANUP" -eq 1 ]; }; then
  assert_active_project_root_can_mutate "pre-mutation" ||
    fail "active release belongs to another project root; refusing to mutate active without MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER=1" 1
  assert_active_release_matches_expected "pre-mutation" ||
    fail "active release differs from expected release before mutation; refusing without MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE=1" 1
  if [ "$DO_DEPLOY" -eq 1 ]; then
    assert_candidate_commit_not_behind_active "pre-mutation" "$GIT_FULL_SHA" ||
      fail "candidate commit is behind or divergent from active release; refusing without MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION=1" 1
  fi
fi

if [ "$DO_DEPLOY" -eq 1 ] || { [ "$CLEANUP_ONLY" -eq 1 ] && [ "$APPLY_CLEANUP" -eq 1 ]; }; then
  acquire_deploy_lock ||
    fail "another MissionD deploy/cleanup owns $DEPLOY_LOCK_PATH; retry after it finishes or remove a verified stale lock" 1
  assert_active_project_root_can_mutate "pre-mutation" ||
    fail "active release belongs to another project root; refusing to mutate active without MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER=1" 1
fi

if [ "$CLEANUP_ONLY" -eq 1 ]; then
  PREVIOUS_ACTIVE="$(resolve_link_target "$ACTIVE_LINK" 2>/dev/null || true)"
  cleanup_old_releases "$APPLY_CLEANUP"
  exit 0
fi

command -v cargo >/dev/null 2>&1 || fail "cargo not on PATH" 1
command -v node >/dev/null 2>&1 || fail "node not on PATH; typed Lisp runtime compile cannot run" 1
command -v dune >/dev/null 2>&1 || fail "dune not on PATH; typed Lisp contract compile cannot run" 1
if [ "$DO_DEPLOY" -eq 1 ]; then
  RELEASE_ID="${MISSIOND_RELEASE_ID:-$(date -u +%Y%m%dT%H%M%SZ)-${GIT_SHA}-${PROFILE}}"
  CANDIDATE_DIR="$RELEASES_DIR/$RELEASE_ID"
  [ ! -e "$CANDIDATE_DIR" ] || fail "candidate release already exists: $CANDIDATE_DIR" 1
  acquire_self_deploy_release_lease ||
    fail "release lease conflict; refusing concurrent MissionD self-deploy" 1
  CANDIDATE_COMPILED_RUNTIME_DIR="$CANDIDATE_DIR/compiled-runtime"
  COMPILED_RUNTIME_DIR="$CANDIDATE_COMPILED_RUNTIME_DIR"
  export MISSIOND_COMPILED_RUNTIME_DIR="$COMPILED_RUNTIME_DIR"
  log "typed-lisp: using release-local compiled runtime dir $COMPILED_RUNTIME_DIR"
else
  BUILD_ONLY_RUNTIME_TMP="$(mktemp -d "${TMPDIR:-/tmp}/missiond-build-only-runtime.XXXXXX")"
  COMPILED_RUNTIME_DIR="$BUILD_ONLY_RUNTIME_TMP/compiled"
  export MISSIOND_COMPILED_RUNTIME_DIR="$COMPILED_RUNTIME_DIR"
  log "typed-lisp: using build-only temporary compiled runtime dir $COMPILED_RUNTIME_DIR"
fi
mkdir -p "$COMPILED_RUNTIME_DIR"
if [ "${MISSIOND_USE_SCCACHE:-0}" = "1" ] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
  log "build: using RUSTC_WRAPPER=$RUSTC_WRAPPER"
elif [ "${MISSIOND_USE_SCCACHE:-0}" = "1" ]; then
  log "build: MISSIOND_USE_SCCACHE=1 but sccache is not installed; continuing without wrapper"
fi
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
log "build: CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
log "typed-lisp: verify V3 contract ABI"
TYPED_LISP_START="$(date +%s)"
if [ "${MISSIOND_DEPLOY_REFRESH_CONTRACTS:-0}" = "1" ]; then
  log "typed-lisp: MISSIOND_DEPLOY_REFRESH_CONTRACTS=1; refreshing generated V3 contract ABI"
  if ! node scripts/project-v3-contracts.mjs --write --json 2>&1 | tail -30; then
    fail "typed Lisp contract ABI refresh failed" 1
  fi
else
  if ! node scripts/project-v3-contracts.mjs --check --json 2>&1 | tail -30; then
    fail "typed Lisp contract ABI verification failed; run node scripts/project-v3-contracts.mjs --write before deploy" 1
  fi
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
capture_launchd_runtime_state
mkdir -p "$CANDIDATE_DIR/bin"
SOURCE_SNAPSHOT_START="$(date +%s)"
CANDIDATE_LAUNCHD_PROJECT_ROOT="$(create_release_source_snapshot "$CANDIDATE_DIR/source")"
record_timing "release-source-snapshot" "$SOURCE_SNAPSHOT_START"
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

ACTIVE_GIT_SHA="$(active_release_git_sha || true)"
cat > "$CANDIDATE_DIR/release-manifest.json" <<EOF
{"schema":"missiond.release-manifest.v1","release_id":"$RELEASE_ID","profile":"$PROFILE","git_sha":"$GIT_SHA","git_full_sha":"$GIT_FULL_SHA","active_git_sha":"${ACTIVE_GIT_SHA:-}","daemon_sha256":"$NEW_HASH","mcp_sha256":"$NEW_MCP_HASH","typed_lisp_runtime":$TYPED_LISP_RUNTIME_MANIFEST,"release_owner_root":"$DEPLOY_OWNER_ROOT","launchd_project_root":"$CANDIDATE_LAUNCHD_PROJECT_ROOT","runtime_dir":"$RUNTIME_DIR","compiled_runtime_dir":"$COMPILED_RUNTIME_DIR","expected_active_release":"${EXPECTED_ACTIVE_RELEASE:-}","previous_active":"${PREVIOUS_ACTIVE:-}","commit_regression_override":"$MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION","created_at":"$(date -u +%Y-%m-%dT%H:%M:%SZ)","source":"scripts/deploy-daemon.sh"}
EOF
write_self_deploy_closure_files "$CANDIDATE_DIR" "blocked" "pending" "complete pre-switch/post-switch smoke before closure"
log "candidate: $CANDIDATE_DIR"

log "pre-switch smoke: candidate MCP initialize"
PRE_SWITCH_SMOKE_START="$(date +%s)"
if ! pre_switch_mcp_smoke "$CANDIDATE_DIR/bin/mission-mcp"; then
  fail "candidate MCP initialize failed before active switch" 3
fi
record_timing "pre-switch-mcp-smoke" "$PRE_SWITCH_SMOKE_START"

assert_active_project_root_can_mutate "pre-switch" ||
  fail "active release changed to another project root before switch; refusing to continue" 4
assert_active_release_matches_expected "pre-switch" ||
  fail "active release changed during deploy build; refusing to overwrite without MISSIOND_DEPLOY_ALLOW_ACTIVE_RELEASE_RACE=1" 4
assert_candidate_commit_not_behind_active "pre-switch" "$GIT_FULL_SHA" ||
  fail "candidate commit is behind or divergent from active release before switch; refusing without MISSIOND_DEPLOY_ALLOW_COMMIT_REGRESSION=1" 4
switch_active_release "$CANDIDATE_DIR"
assert_active_release_owned "post-switch" ||
  fail "deploy ownership guard failed after active switch; refusing to continue" 4
ensure_default_mcp_config

LAUNCHD_PROJECT_ROOT="$CANDIDATE_LAUNCHD_PROJECT_ROOT"
KICKSTART_START="$(date +%s)"
if ! restart_daemon_supervisor; then
  rollback_with_smoke "$PREVIOUS_ACTIVE" || true
  fail "launchctl reload/kickstart failed; rollback attempted" 4
fi
assert_active_release_owned "post-launchd" ||
  fail "deploy ownership guard failed after launchd restart; refusing to continue" 4
assert_launchd_runtime_owned "post-launchd" ||
  fail "deploy ownership guard failed: launchd runtime roots do not match active release" 4
record_timing "launchd-kickstart" "$KICKSTART_START"

SOCKET_WAIT_START="$(date +%s)"
if ! wait_for_socket; then
  rollback_with_smoke "$PREVIOUS_ACTIVE" || true
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
  rollback_with_smoke "$PREVIOUS_ACTIVE" || true
  fail "smoke check failed; rollback attempted" 6
fi
assert_active_release_owned "post-mcp-smoke" ||
  fail "deploy ownership guard failed after MCP smoke; refusing to continue" 6
assert_launchd_runtime_owned "post-mcp-smoke" ||
  fail "deploy ownership guard failed after MCP smoke: launchd roots drifted" 6
record_timing "post-switch-mcp-smoke" "$POST_SMOKE_START"

JARVIS_SLOT_START="$(date +%s)"
if ! post_switch_jarvis_slot_ensure; then
  rollback_with_smoke "$PREVIOUS_ACTIVE" || true
  fail "Jarvis default slot ensure failed; rollback attempted" 6
fi
assert_active_release_owned "post-jarvis-smoke" ||
  fail "deploy ownership guard failed after Jarvis smoke; refusing to continue" 6
record_timing "post-switch-jarvis-slot-ensure" "$JARVIS_SLOT_START"

CLEANUP_START="$(date +%s)"
cleanup_old_releases 1
cleanup_repo_runtime_cache
assert_active_release_owned "post-cleanup" ||
  fail "deploy ownership guard failed after cleanup; refusing to report success" 6
assert_launchd_runtime_owned "post-cleanup" ||
  fail "deploy ownership guard failed after cleanup: launchd roots drifted" 6
record_timing "cleanup" "$CLEANUP_START"
write_self_deploy_closure_files "$CANDIDATE_DIR" "success" "passed" "release closed"
print_timing_summary
log "deploy: done. active_release=$RELEASE_ID previous=${PREVIOUS_ACTIVE:-none}"
