#!/usr/bin/env bash
# Bootstrap a managed macOS node for MissionD local build/deploy work.
# This script is idempotent and intentionally small: it installs Homebrew
# when missing, installs libpq for psql diagnostics/backfills, wires PATH, and
# verifies psql. It does not install MissionD itself.

set -euo pipefail

BREW_PREFIX="${MISSIOND_HOMEBREW_PREFIX:-/opt/homebrew}"
BREW_BIN="${BREW_PREFIX}/bin/brew"
ZSHENV="${MISSIOND_MANAGED_MAC_ZSHENV:-${HOME}/.zshenv}"
MARKER_BEGIN="# >>> missiond managed mac node path >>>"
MARKER_END="# <<< missiond managed mac node path <<<"

find_brew() {
  if command -v brew >/dev/null 2>&1; then
    command -v brew
    return 0
  fi
  if [ -x "$BREW_BIN" ]; then
    printf '%s\n' "$BREW_BIN"
    return 0
  fi
  if [ -x /usr/local/bin/brew ]; then
    printf '%s\n' /usr/local/bin/brew
    return 0
  fi
  return 1
}

install_homebrew() {
  echo "install Homebrew"
  NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
}

ensure_path_block() {
  if [ "${MISSIOND_SKIP_SHELL_PATH_BOOTSTRAP:-0}" = "1" ]; then
    return
  fi

  mkdir -p "$(dirname "$ZSHENV")"
  touch "$ZSHENV"

  local tmp
  tmp="$(mktemp)"
  awk -v begin="$MARKER_BEGIN" -v end="$MARKER_END" '
    $0 == begin { skip = 1; next }
    $0 == end { skip = 0; next }
    skip != 1 { print }
  ' "$ZSHENV" > "$tmp"
  cat >> "$tmp" <<'EOF'
# >>> missiond managed mac node path >>>
export PATH="/opt/homebrew/opt/libpq/bin:/usr/local/opt/libpq/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
# <<< missiond managed mac node path <<<
EOF
  mv "$tmp" "$ZSHENV"
}

if ! BREW="$(find_brew)"; then
  install_homebrew
  BREW="$(find_brew)"
fi

export PATH="/opt/homebrew/opt/libpq/bin:/usr/local/opt/libpq/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

echo "brew install libpq"
HOMEBREW_NO_AUTO_UPDATE="${HOMEBREW_NO_AUTO_UPDATE:-1}" "$BREW" install libpq
"$BREW" link --force libpq >/dev/null 2>&1 || true

ensure_path_block

psql --version
