#!/usr/bin/env bash
# Build & test driver for projectmind.
#
# Same logic the GitHub Actions workflow runs. Reproduce CI locally with:
#
#   ./scripts/ci.sh all      # fmt + clippy + tests + doctests
#   ./scripts/ci.sh check    # fmt + clippy only
#   ./scripts/ci.sh test     # tests + doctests only
#
# Used by .github/workflows/{ci,release}.yml so the workflow files stay thin.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

# CI-friendly defaults — overridable from the environment.
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

usage() {
    cat <<EOF
Usage: $(basename "$0") <command> [args]

Commands:
  check                       cargo fmt --check + cargo clippy --workspace
  test                        cargo test --workspace --all-targets + --doc
  install-deps                Install Linux Tauri build deps via apt (no-op on macOS)
  release-build [<target>]    cargo build --release --bin projectmind-mcp [--target <target>]
  release-smoke               release-build + stdio JSON-RPC ping against the binary
  release-package <target> <suffix>
                              tar.gz + sha256 packaging for the MCP server binary
  app-build [<target>]        Build the Tauri desktop app bundle for the host
                              platform (.app/.dmg on macOS, .deb/.AppImage on Linux,
                              .msi/.exe on Windows). Optional Rust target triple
                              for cross-arch builds (e.g. universal-apple-darwin).
  app-package <target> <suffix>
                              Collect every Tauri bundle artefact under
                              target/<target>/release/bundle into
                              projectmind-app-<suffix>.tar.gz + .sha256.
  all                         check + test
EOF
}

cmd_check() {
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
}

cmd_test() {
    cargo test --workspace --all-targets
    cargo test --workspace --doc
}

cmd_install_deps() {
    if [[ "$(uname -s)" != "Linux" ]]; then
        echo "install-deps: skipped (not Linux)"
        return 0
    fi
    sudo apt-get update
    sudo apt-get install -y \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libsoup-3.0-dev \
        libjavascriptcoregtk-4.1-dev \
        patchelf
}

cmd_release_build() {
    local target="${1:-}"
    if [[ -n "$target" ]]; then
        cargo build --release --bin projectmind-mcp --target "$target"
    else
        cargo build --release --bin projectmind-mcp
    fi
}

cmd_release_smoke() {
    cmd_release_build
    local bin="target/release/projectmind-mcp"
    local tmp
    tmp="$(mktemp -t projectmind-smoke.XXXXXX)"
    trap 'rm -f "$tmp"' RETURN

    # Pipe stays open until the server reads EOF and exits cleanly. Using `grep -q` in
    # the pipeline causes a broken-pipe race because grep exits on first match while the
    # server is still writing — capture the full response to a file and grep that.
    printf '%s\n' \
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"ci","version":"1.0"}}}' \
        '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
        | "$bin" > "$tmp"

    if ! grep -q '"name":"projectmind-mcp"' "$tmp"; then
        echo "release-smoke: server name not found in response" >&2
        cat "$tmp" >&2
        exit 1
    fi
    if ! grep -q '"name":"open_repo"' "$tmp"; then
        echo "release-smoke: tools/list did not include open_repo" >&2
        cat "$tmp" >&2
        exit 1
    fi
    echo "release-smoke: ok"
}

cmd_release_package() {
    local target="${1:?target required}"
    local suffix="${2:?asset suffix required}"
    local artifact_name="projectmind-mcp"
    local bin_path="target/${target}/release/${artifact_name}"
    local archive="${artifact_name}-${suffix}.tar.gz"

    if [[ ! -x "$bin_path" ]]; then
        echo "release-package: missing $bin_path — run release-build first" >&2
        exit 1
    fi

    # Bundle binary + LICENSE + README into a flat archive.
    tar czf "$archive" \
        -C "$(dirname "$bin_path")" "$(basename "$bin_path")" \
        -C "$ROOT_DIR" LICENSE README.md
    shasum -a 256 "$archive" > "$archive.sha256"
    echo "release-package: $archive ($(wc -c <"$archive") bytes)"
}

cmd_app_build() {
    local target="${1:-}"
    cd "$ROOT_DIR/app"
    if [[ ! -d node_modules ]]; then
        echo "app-build: installing npm deps (first run)"
        npm install
    fi
    if [[ -n "$target" ]]; then
        echo "app-build: tauri build --target $target"
        npm run tauri -- build -- --target "$target"
    else
        echo "app-build: tauri build (host target)"
        npm run tauri -- build
    fi
    cd "$ROOT_DIR"
}

cmd_app_package() {
    local target="${1:?target triple required, e.g. aarch64-apple-darwin}"
    local suffix="${2:?asset suffix required, e.g. macos-arm64}"
    local archive="projectmind-app-${suffix}.tar.gz"
    local bundle_dir="target/${target}/release/bundle"

    if [[ ! -d "$bundle_dir" ]]; then
        echo "app-package: missing $bundle_dir — run app-build first" >&2
        exit 1
    fi

    # Tauri scatters bundle artefacts across format-specific subdirs
    # (bundle/dmg/, bundle/macos/, bundle/deb/, bundle/appimage/, bundle/msi/, …).
    # We pick whatever distributable formats actually got produced and pack them
    # plus LICENSE + README into one archive. This keeps the workflow simple:
    # ONE artefact per target, asset_suffix telling Mac/Linux/Win apart.
    local bundles=()
    while IFS= read -r f; do
        bundles+=("$f")
    done < <(find "$bundle_dir" -type f \
        \( -name "*.dmg" -o -name "*.app.tar.gz" -o -name "*.deb" \
           -o -name "*.AppImage" -o -name "*.msi" -o -name "*.exe" \
           -o -name "*.app" \) 2>/dev/null | sort)

    if [[ ${#bundles[@]} -eq 0 ]]; then
        echo "app-package: no bundle artefacts found under $bundle_dir" >&2
        find "$bundle_dir" -maxdepth 3 -type f >&2 || true
        exit 1
    fi

    # `tar -C <dir> file` requires file as a relative path inside <dir>.
    # Build a flat archive: every artefact at the archive root.
    local args=()
    for f in "${bundles[@]}"; do
        args+=( -C "$(dirname "$f")" "$(basename "$f")" )
    done
    tar czf "$archive" "${args[@]}" -C "$ROOT_DIR" LICENSE README.md
    shasum -a 256 "$archive" > "$archive.sha256"
    local size_h
    size_h="$(du -h "$archive" | cut -f1)"
    echo "app-package: $archive ($size_h, ${#bundles[@]} bundle file(s))"
}

case "${1:-}" in
    check)           shift; cmd_check "$@" ;;
    test)            shift; cmd_test "$@" ;;
    install-deps)    shift; cmd_install_deps "$@" ;;
    release-build)   shift; cmd_release_build "$@" ;;
    release-smoke)   shift; cmd_release_smoke "$@" ;;
    release-package) shift; cmd_release_package "$@" ;;
    app-build)       shift; cmd_app_build "$@" ;;
    app-package)     shift; cmd_app_package "$@" ;;
    all)             cmd_check; cmd_test ;;
    -h|--help|help|"") usage ;;
    *) echo "unknown command: $1" >&2; usage; exit 2 ;;
esac
