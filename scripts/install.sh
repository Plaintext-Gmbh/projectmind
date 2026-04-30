#!/bin/sh
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# One-shot installer for ProjectMind on macOS and Linux. Downloads the
# pre-built bundle for the latest release matching the current host and
# drops the desktop app + MCP server in standard locations.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Plaintext-Gmbh/projectmind/master/scripts/install.sh | sh
#
# Environment overrides:
#   PM_VERSION=v1.2.3  pin a specific release tag (default: latest)
#   PM_PREFIX=/path    install MCP binary somewhere other than ~/.local/bin
#                      (or /usr/local/bin if the script is run as root)
#   PM_NO_APP=1        skip the desktop app, only install the MCP server
#   PM_NO_MCP=1        skip the MCP server, only install the desktop app

set -eu

REPO="Plaintext-Gmbh/projectmind"
VERSION="${PM_VERSION:-latest}"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '\033[36m::\033[0m %s\n' "$*"; }
warn() { printf '\033[33m!!\033[0m %s\n' "$*"; }
fail() { printf '\033[31mxx\033[0m %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1"
}

need uname
need tar
if command -v curl >/dev/null 2>&1; then
    DL='curl -fsSL'
elif command -v wget >/dev/null 2>&1; then
    DL='wget -qO-'
else
    fail "need either curl or wget to download release assets"
fi

# ---- detect OS + arch ------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        APP_SUFFIX="macos-universal"
        case "$ARCH" in
            arm64)  MCP_SUFFIX="macos-arm64" ;;
            x86_64) MCP_SUFFIX="macos-x86_64" ;;
            *) fail "unsupported macOS arch: $ARCH" ;;
        esac
        ;;
    Linux)
        case "$ARCH" in
            x86_64)  APP_SUFFIX="linux-x86_64";  MCP_SUFFIX="linux-x86_64" ;;
            aarch64) fail "linux-arm64 builds aren't published yet — open an issue if you need them" ;;
            *) fail "unsupported Linux arch: $ARCH" ;;
        esac
        ;;
    *)
        fail "unsupported OS: $OS (use scripts/install.ps1 on Windows)"
        ;;
esac

# ---- pick install prefixes -------------------------------------------------
if [ "$(id -u)" -eq 0 ]; then
    PREFIX="${PM_PREFIX:-/usr/local/bin}"
    APP_DEST_MAC="/Applications"
    APP_DEST_LINUX="/opt/projectmind"
else
    PREFIX="${PM_PREFIX:-$HOME/.local/bin}"
    APP_DEST_MAC="/Applications"   # macOS still goes to /Applications by convention
    APP_DEST_LINUX="$HOME/.local/share/projectmind"
fi

mkdir -p "$PREFIX"

# ---- resolve version -------------------------------------------------------
RELEASE_API="https://api.github.com/repos/${REPO}/releases"
if [ "$VERSION" = "latest" ]; then
    info "resolving latest release tag"
    TAG="$($DL "${RELEASE_API}/latest" \
        | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' \
        | head -n1)"
    [ -n "${TAG:-}" ] || fail "could not parse latest release tag from GitHub API"
else
    TAG="$VERSION"
fi
info "version: $TAG"

DOWNLOAD_BASE="https://github.com/${REPO}/releases/download/${TAG}"

TMP="$(mktemp -d -t projectmind-install.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

download() {
    asset="$1"
    info "downloading $asset"
    case "$DL" in
        curl*) curl -fsSL "${DOWNLOAD_BASE}/${asset}" -o "${TMP}/${asset}" ;;
        wget*) wget -q "${DOWNLOAD_BASE}/${asset}" -O "${TMP}/${asset}" ;;
    esac
}

# ---- MCP server ------------------------------------------------------------
if [ "${PM_NO_MCP:-0}" != "1" ]; then
    MCP_ARCHIVE="projectmind-mcp-${MCP_SUFFIX}.tar.gz"
    download "$MCP_ARCHIVE"
    mkdir -p "${TMP}/mcp"
    tar xzf "${TMP}/${MCP_ARCHIVE}" -C "${TMP}/mcp"
    install_target="${PREFIX}/projectmind-mcp"
    cp "${TMP}/mcp/projectmind-mcp" "$install_target"
    chmod +x "$install_target"
    info "installed: $install_target"
else
    warn "PM_NO_MCP=1 — skipping MCP server"
fi

# ---- Desktop app -----------------------------------------------------------
if [ "${PM_NO_APP:-0}" != "1" ]; then
    APP_ARCHIVE="projectmind-app-${APP_SUFFIX}.tar.gz"
    download "$APP_ARCHIVE"
    mkdir -p "${TMP}/app"
    tar xzf "${TMP}/${APP_ARCHIVE}" -C "${TMP}/app"
    case "$OS" in
        Darwin)
            # macOS bundle ships as a .dmg or .app.tar.gz inside the archive.
            # Mount the dmg if present; otherwise look for .app/.
            dmg="$(find "${TMP}/app" -maxdepth 2 -name '*.dmg' | head -n1 || true)"
            app_dir="$(find "${TMP}/app" -maxdepth 2 -name '*.app' -type d | head -n1 || true)"
            if [ -n "$dmg" ]; then
                info "mounting $dmg"
                mp="$(hdiutil attach "$dmg" -nobrowse -readonly | tail -n1 | awk '{print $3}')"
                src_app="$(find "$mp" -maxdepth 1 -name '*.app' | head -n1)"
                [ -n "$src_app" ] || { hdiutil detach "$mp" -quiet; fail "no .app found in mounted dmg"; }
                rm -rf "${APP_DEST_MAC}/$(basename "$src_app")"
                cp -R "$src_app" "$APP_DEST_MAC/"
                hdiutil detach "$mp" -quiet
                info "installed: ${APP_DEST_MAC}/$(basename "$src_app")"
            elif [ -n "$app_dir" ]; then
                rm -rf "${APP_DEST_MAC}/$(basename "$app_dir")"
                cp -R "$app_dir" "$APP_DEST_MAC/"
                info "installed: ${APP_DEST_MAC}/$(basename "$app_dir")"
            else
                warn "no .dmg / .app found in $APP_ARCHIVE — desktop app skipped"
            fi
            ;;
        Linux)
            mkdir -p "$APP_DEST_LINUX"
            appimage="$(find "${TMP}/app" -maxdepth 2 -name '*.AppImage' | head -n1 || true)"
            deb="$(find "${TMP}/app" -maxdepth 2 -name '*.deb' | head -n1 || true)"
            if [ -n "$appimage" ]; then
                cp "$appimage" "$APP_DEST_LINUX/projectmind.AppImage"
                chmod +x "$APP_DEST_LINUX/projectmind.AppImage"
                ln -sf "$APP_DEST_LINUX/projectmind.AppImage" "$PREFIX/projectmind"
                info "installed: $APP_DEST_LINUX/projectmind.AppImage"
                info "shortcut:  $PREFIX/projectmind"
            elif [ -n "$deb" ] && command -v dpkg >/dev/null 2>&1; then
                info "installing $deb via dpkg (sudo may prompt)"
                if [ "$(id -u)" -eq 0 ]; then
                    dpkg -i "$deb" || apt-get -f install -y
                else
                    sudo dpkg -i "$deb" || sudo apt-get -f install -y
                fi
                info "installed via apt: projectmind"
            else
                warn "no .AppImage or .deb found in $APP_ARCHIVE — desktop app skipped"
            fi
            ;;
    esac
else
    warn "PM_NO_APP=1 — skipping desktop app"
fi

bold ""
bold "ProjectMind $TAG installed."
case "$OS" in
    Darwin) info "Launch from Spotlight or open '${APP_DEST_MAC}/ProjectMind.app'." ;;
    Linux)  info "Launch with 'projectmind' (if it's on your PATH) or run the AppImage directly." ;;
esac
if [ "${PM_NO_MCP:-0}" != "1" ]; then
    info "MCP server: $PREFIX/projectmind-mcp"
    info "Add to your LLM CLI's mcp config — see https://github.com/${REPO}/#readme"
fi
