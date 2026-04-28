#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# Convenience installer for Ubuntu / Debian. Installs prerequisites, builds
# the MCP server (and optionally the Tauri shell), and prints a ready-made
# .mcp.json snippet for Claude Code.
#
# Usage:
#   ./scripts/install-ubuntu.sh             # MCP server only (recommended)
#   ./scripts/install-ubuntu.sh --with-app  # also build the Tauri shell

set -euo pipefail

WITH_APP=0
for arg in "$@"; do
    case "$arg" in
        --with-app) WITH_APP=1 ;;
        -h|--help)
            sed -n '5,15p' "$0" | sed 's/^# //; s/^#$//'
            exit 0
            ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
info()  { printf '\033[36m::\033[0m %s\n' "$*"; }
warn()  { printf '\033[33m!!\033[0m %s\n' "$*"; }
ok()    { printf '\033[32m✓\033[0m %s\n' "$*"; }

bold "plaintext-ide installer for Ubuntu / Debian"
echo

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v sudo >/dev/null 2>&1; then
    warn "sudo not found; assuming you can install packages directly"
    SUDO=""
else
    SUDO="sudo"
fi

info "Installing build prerequisites"
$SUDO apt-get update -qq
$SUDO apt-get install -y --no-install-recommends \
    build-essential pkg-config cmake git curl libssl-dev

if [[ $WITH_APP -eq 1 ]]; then
    info "Installing Tauri prerequisites"
    $SUDO apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
        librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev patchelf
fi

if ! command -v cargo >/dev/null 2>&1; then
    info "Installing Rust toolchain via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile default
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
else
    ok "Rust already installed: $(cargo --version)"
fi

info "Building plaintext-ide-mcp (release)"
cargo build --release --bin plaintext-ide-mcp

MCP_BIN="$REPO_ROOT/target/release/plaintext-ide-mcp"
ok "Built: $MCP_BIN"

if [[ $WITH_APP -eq 1 ]]; then
    if ! command -v pnpm >/dev/null 2>&1; then
        info "Installing pnpm"
        curl -fsSL https://get.pnpm.io/install.sh | sh -
        export PATH="$HOME/.local/share/pnpm:$PATH"
    fi
    info "Building Tauri shell"
    (cd app && pnpm install && pnpm tauri build)
    ok "App built — see app/src-tauri/target/release/bundle/"
fi

echo
bold "Add this to your Claude Code configuration (.mcp.json)"
cat <<EOF
{
  "mcpServers": {
    "plaintext-ide": {
      "type": "stdio",
      "command": "$MCP_BIN",
      "env": { "PLAINTEXT_IDE_LOG": "info" }
    }
  }
}
EOF
echo
ok "Done. Restart Claude Code and try: 'open the plaintext-app repo and list services'."
