#!/usr/bin/env bash
# Bring up MissionD local provider services in a reproducible way.
#
# This manages the private local development wiring for:
#   - xjp-memory   -> http://127.0.0.1:8091
#   - xjp-eventhub -> http://127.0.0.1:8092
#
# Usage:
#   scripts/manage-local-providers.sh install   # build, ensure DBs, write plists, restart providers + MissionD
#   scripts/manage-local-providers.sh restart   # restart provider LaunchAgents and MissionD
#   scripts/manage-local-providers.sh smoke     # verify provider HTTP endpoints
#   scripts/manage-local-providers.sh status    # print launchd and HTTP status
#   scripts/manage-local-providers.sh stop      # stop provider LaunchAgents only
#
# Environment overrides:
#   XJP_BACKEND_ROOT             default: /Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend
#   MISSIOND_LAUNCHD_PLIST       default: ~/Library/LaunchAgents/com.missiond.daemon.plist
#   MISSIOND_LAUNCHD_LABEL       default: com.missiond.daemon
#   XJP_MEMORY_DATABASE_URL      default: postgres://$USER@localhost/xjp_memory
#   XJP_EVENTHUB_DATABASE_URL    default: postgres://$USER@localhost/xjp_eventhub

set -euo pipefail

ACTION="${1:-status}"
UID_NUM="$(id -u)"
XJP_BACKEND_ROOT="${XJP_BACKEND_ROOT:-/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend}"
LAUNCH_AGENTS_DIR="${HOME}/Library/LaunchAgents"
MISSIOND_LAUNCHD_PLIST="${MISSIOND_LAUNCHD_PLIST:-${LAUNCH_AGENTS_DIR}/com.missiond.daemon.plist}"
MISSIOND_LAUNCHD_LABEL="${MISSIOND_LAUNCHD_LABEL:-com.missiond.daemon}"

MEMORY_LABEL="${XJP_MEMORY_LAUNCHD_LABEL:-com.xjp.memory.provider}"
EVENTHUB_LABEL="${XJP_EVENTHUB_LAUNCHD_LABEL:-com.xjp.eventhub.provider}"
MEMORY_PLIST="${LAUNCH_AGENTS_DIR}/${MEMORY_LABEL}.plist"
EVENTHUB_PLIST="${LAUNCH_AGENTS_DIR}/${EVENTHUB_LABEL}.plist"
MEMORY_PORT="${XJP_MEMORY_PORT:-8091}"
EVENTHUB_PORT="${XJP_EVENTHUB_PORT:-8092}"
MEMORY_URL="http://127.0.0.1:${MEMORY_PORT}"
EVENTHUB_URL="http://127.0.0.1:${EVENTHUB_PORT}"
MEMORY_DB_URL="${XJP_MEMORY_DATABASE_URL:-postgres://${USER}@localhost/xjp_memory}"
EVENTHUB_DB_URL="${XJP_EVENTHUB_DATABASE_URL:-postgres://${USER}@localhost/xjp_eventhub}"

MEMORY_BIN="${XJP_BACKEND_ROOT}/target/debug/xjp-memory"
EVENTHUB_BIN="${XJP_BACKEND_ROOT}/target/debug/xjp-eventhub"

log() { printf '[local-providers] %s\n' "$*" >&2; }
die() { printf '[local-providers] FAIL: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,24p' "$0"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

build_services() {
  [ -d "$XJP_BACKEND_ROOT" ] || die "XJP_BACKEND_ROOT not found: $XJP_BACKEND_ROOT"
  log "build: xjp-memory + xjp-eventhub in $XJP_BACKEND_ROOT"
  (cd "$XJP_BACKEND_ROOT" && cargo build -p xjp-memory -p xjp-eventhub)
  [ -x "$MEMORY_BIN" ] || die "memory binary missing after build: $MEMORY_BIN"
  [ -x "$EVENTHUB_BIN" ] || die "eventhub binary missing after build: $EVENTHUB_BIN"
}

ensure_database() {
  local name="$1"
  require_command psql
  require_command createdb
  if psql -d postgres -Atqc "SELECT 1 FROM pg_database WHERE datname = '${name}'" | grep -q '^1$'; then
    log "database exists: $name"
    return 0
  fi
  log "create database: $name"
  createdb "$name"
}

write_provider_plist() {
  local plist="$1"
  local label="$2"
  local bin="$3"
  local port="$4"
  local db_env_name="$5"
  local db_url="$6"
  local provider_id_env="$7"
  local provider_id="$8"
  local out_log="${HOME}/Library/Logs/${label}.out.log"
  local err_log="${HOME}/Library/Logs/${label}.err.log"
  mkdir -p "$LAUNCH_AGENTS_DIR" "${HOME}/Library/Logs"
  log "write plist: $plist"
  cat >"$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${bin}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${XJP_BACKEND_ROOT}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PORT</key>
    <string>${port}</string>
    <key>${db_env_name}</key>
    <string>${db_url}</string>
    <key>${provider_id_env}</key>
    <string>${provider_id}</string>
    <key>RUST_LOG</key>
    <string>info</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>${out_log}</string>
  <key>StandardErrorPath</key>
  <string>${err_log}</string>
</dict>
</plist>
EOF
  plutil -lint "$plist" >/dev/null
}

bootout_label() {
  local label="$1"
  launchctl bootout "gui/${UID_NUM}/${label}" >/dev/null 2>&1 || true
}

bootstrap_plist() {
  local plist="$1"
  local label="$2"
  local attempt output
  bootout_label "$label"
  launchctl bootout "gui/${UID_NUM}" "$plist" >/dev/null 2>&1 || true
  sleep 1
  for attempt in 1 2 3; do
    log "bootstrap: $label (attempt ${attempt})"
    if output="$(launchctl bootstrap "gui/${UID_NUM}" "$plist" 2>&1)"; then
      launchctl kickstart -k "gui/${UID_NUM}/${label}" >/dev/null 2>&1 || true
      return 0
    fi
    printf '%s\n' "$output" >&2
    sleep "$attempt"
  done
  die "launchctl bootstrap failed for ${label}; inspect ${plist} with plutil and launchctl print gui/${UID_NUM}/${label}"
}

plist_set_env() {
  local plist="$1"
  local key="$2"
  local value="$3"
  [ -f "$plist" ] || die "MissionD LaunchAgent plist not found: $plist"
  /usr/libexec/PlistBuddy -c "Add :EnvironmentVariables dict" "$plist" >/dev/null 2>&1 || true
  if /usr/libexec/PlistBuddy -c "Print :EnvironmentVariables:${key}" "$plist" >/dev/null 2>&1; then
    /usr/libexec/PlistBuddy -c "Set :EnvironmentVariables:${key} ${value}" "$plist"
  else
    /usr/libexec/PlistBuddy -c "Add :EnvironmentVariables:${key} string ${value}" "$plist"
  fi
}

wire_missiond_env() {
  log "wire MissionD provider env: $MISSIOND_LAUNCHD_PLIST"
  plist_set_env "$MISSIOND_LAUNCHD_PLIST" "MISSIOND_MEMORY_PROVIDER_URL" "$MEMORY_URL"
  plist_set_env "$MISSIOND_LAUNCHD_PLIST" "MISSIOND_MEMORY_PROVIDER_MODE" "xjp-memory"
  plist_set_env "$MISSIOND_LAUNCHD_PLIST" "MISSIOND_EVENTHUB_URL" "$EVENTHUB_URL"
  plist_set_env "$MISSIOND_LAUNCHD_PLIST" "MISSIOND_EVENTHUB_MODE" "xjp-eventhub"
  plutil -lint "$MISSIOND_LAUNCHD_PLIST" >/dev/null
}

restart_missiond() {
  local attempt output
  [ -f "$MISSIOND_LAUNCHD_PLIST" ] || die "MissionD LaunchAgent plist not found: $MISSIOND_LAUNCHD_PLIST"
  bootout_label "$MISSIOND_LAUNCHD_LABEL"
  launchctl bootout "gui/${UID_NUM}" "$MISSIOND_LAUNCHD_PLIST" >/dev/null 2>&1 || true
  sleep 1
  for attempt in 1 2 3; do
    log "bootstrap MissionD: $MISSIOND_LAUNCHD_LABEL (attempt ${attempt})"
    if output="$(launchctl bootstrap "gui/${UID_NUM}" "$MISSIOND_LAUNCHD_PLIST" 2>&1)"; then
      launchctl kickstart -k "gui/${UID_NUM}/${MISSIOND_LAUNCHD_LABEL}" >/dev/null 2>&1 || true
      return 0
    fi
    printf '%s\n' "$output" >&2
    sleep "$attempt"
  done
  die "launchctl bootstrap failed for ${MISSIOND_LAUNCHD_LABEL}; inspect ${MISSIOND_LAUNCHD_PLIST}"
}

http_json() {
  local url="$1"
  curl -fsS --max-time 5 "$url"
}

smoke() {
  log "smoke: xjp-memory provider_status"
  http_json "${MEMORY_URL}/v1/memory/provider_status" | grep -q '"provider_id"'
  http_json "${MEMORY_URL}/v1/memory/provider_status" | grep -q '"postgres-durable"'
  log "smoke: xjp-eventhub status"
  http_json "${EVENTHUB_URL}/v1/eventhub/status" | grep -q '"provider_id"'
  http_json "${EVENTHUB_URL}/v1/eventhub/status" | grep -q '"postgres-durable"'
  log "smoke OK"
}

status() {
  for label in "$MEMORY_LABEL" "$EVENTHUB_LABEL" "$MISSIOND_LAUNCHD_LABEL"; do
    if launchctl print "gui/${UID_NUM}/${label}" >/dev/null 2>&1; then
      printf '%s: loaded\n' "$label"
      launchctl print "gui/${UID_NUM}/${label}" | sed -n '1,18p'
    else
      printf '%s: not-loaded\n' "$label"
    fi
  done
  printf 'xjp-memory: '
  http_json "${MEMORY_URL}/v1/memory/provider_status" || true
  printf '\nxjp-eventhub: '
  http_json "${EVENTHUB_URL}/v1/eventhub/status" || true
  printf '\n'
}

install() {
  require_command cargo
  require_command launchctl
  require_command plutil
  build_services
  ensure_database xjp_memory
  ensure_database xjp_eventhub
  write_provider_plist "$MEMORY_PLIST" "$MEMORY_LABEL" "$MEMORY_BIN" "$MEMORY_PORT" "XJP_MEMORY_DATABASE_URL" "$MEMORY_DB_URL" "XJP_MEMORY_PROVIDER_ID" "xjp-memory-local-launchd"
  write_provider_plist "$EVENTHUB_PLIST" "$EVENTHUB_LABEL" "$EVENTHUB_BIN" "$EVENTHUB_PORT" "XJP_EVENTHUB_DATABASE_URL" "$EVENTHUB_DB_URL" "XJP_EVENTHUB_PROVIDER_ID" "xjp-eventhub-local-launchd"
  bootstrap_plist "$MEMORY_PLIST" "$MEMORY_LABEL"
  bootstrap_plist "$EVENTHUB_PLIST" "$EVENTHUB_LABEL"
  wire_missiond_env
  restart_missiond
  sleep 2
  smoke
}

case "$ACTION" in
  install) install ;;
  restart)
    bootstrap_plist "$MEMORY_PLIST" "$MEMORY_LABEL"
    bootstrap_plist "$EVENTHUB_PLIST" "$EVENTHUB_LABEL"
    restart_missiond
    sleep 2
    smoke
    ;;
  smoke) smoke ;;
  status) status ;;
  stop)
    bootout_label "$MEMORY_LABEL"
    bootout_label "$EVENTHUB_LABEL"
    ;;
  -h|--help|help) usage ;;
  *) usage; die "unknown action: $ACTION" ;;
esac
