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
                              Stages the MCP binary as a Tauri sidecar first so
                              the bundle ships both binaries.
  stage-mcp-sidecar [<target>]
                              Build projectmind-mcp for the given target (host
                              triple if omitted; per-arch + lipo for
                              universal-apple-darwin) and copy it to
                              app/src-tauri/binaries/ where tauri-bundler's
                              externalBin lookup finds it.
  app-package <target> <suffix>
                              Collect every Tauri bundle artefact under
                              target/<target>/release/bundle into
                              projectmind-app-<suffix>.tar.gz + .sha256.
  all                         check + test
EOF
}

# Cross-platform sha256 helper. macOS has shasum, Linux has sha256sum,
# Windows runners under Git Bash have either depending on the runner image.
sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file"
    else
        shasum -a 256 "$file"
    fi
}

cmd_check() {
    cargo fmt --all -- --check
    local cargo_args=(--workspace --all-targets)
    if [[ "${PROJECTMIND_SKIP_TAURI_APP:-}" == "1" ]]; then
        cargo_args+=(--exclude projectmind-app)
    else
        ensure_sidecar_placeholder
    fi
    cargo clippy "${cargo_args[@]}" -- -D warnings
}

cmd_test() {
    local cargo_args=(--workspace)
    if [[ "${PROJECTMIND_SKIP_TAURI_APP:-}" == "1" ]]; then
        cargo_args+=(--exclude projectmind-app)
    else
        ensure_sidecar_placeholder
    fi
    cargo test "${cargo_args[@]}" --all-targets
    cargo test "${cargo_args[@]}" --doc
}

# tauri-build's build script validates `bundle.externalBin` paths on
# every cargo invocation that touches the projectmind-app crate, even
# `cargo check` / `clippy`. Stage an empty placeholder so check/test
# don't have to do a full release build of the MCP binary just to
# satisfy that lookup. Real release builds replace the placeholder via
# `stage-mcp-sidecar` before `tauri build`.
ensure_sidecar_placeholder() {
    local sidecar_dir="$ROOT_DIR/app/src-tauri/binaries"
    local host_triple
    host_triple="$(rustc -vV | sed -n 's/^host: //p')"
    local ext=""
    case "$host_triple" in *windows*) ext=".exe" ;; esac
    local path="$sidecar_dir/projectmind-mcp-${host_triple}${ext}"
    if [[ -f "$path" ]]; then
        return
    fi
    mkdir -p "$sidecar_dir"
    : > "$path"
    chmod +x "$path" 2>/dev/null || true
}

cmd_install_deps() {
    if [[ "$(uname -s)" != "Linux" ]]; then
        echo "install-deps: skipped (not Linux)"
        return 0
    fi
    export DEBIAN_FRONTEND=noninteractive
    sudo -E apt-get -o Dpkg::Lock::Timeout=120 update
    sudo -E apt-get -o Dpkg::Lock::Timeout=120 install -y --no-install-recommends \
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
    sha256 "$archive" > "$archive.sha256"
    echo "release-package: $archive ($(wc -c <"$archive") bytes)"
}

cmd_app_build() {
    local target="${1:-}"
    # Stage the MCP server as a Tauri sidecar before kicking off the
    # bundle build. tauri.conf.json declares `externalBin` so the bundler
    # picks up `app/src-tauri/binaries/projectmind-mcp-<triple>{ext}` and
    # ships it next to the GUI binary inside the .app/.deb/.msi.
    cmd_stage_mcp_sidecar "$target"

    cd "$ROOT_DIR/app"
    if [[ ! -d node_modules ]]; then
        echo "app-build: installing js deps (first run)"
        pnpm install --frozen-lockfile
    fi
    if [[ -n "$target" ]]; then
        echo "app-build: tauri build --target $target"
        pnpm tauri build --target "$target"
    else
        echo "app-build: tauri build (host target)"
        pnpm tauri build
    fi
    cd "$ROOT_DIR"

    cmd_macos_stabilize_bundle_id "$target"
}

# Tauri's bundler leaves the .app with the linker's auto ad-hoc signature,
# whose Identifier is "<crate_name>-<random_hash>" instead of the configured
# bundle.identifier. macOS keys TCC permissions (Schreibtisch, Documents, …)
# off (Identifier, CodeRequirement) — a wandering Identifier means the
# permission prompt re-fires after every rebuild / auto-update.
#
# Re-sign ad-hoc with --identifier set to the value from tauri.conf.json so
# the Identifier half of that tuple is stable across rebuilds. This is the
# best we can do without an Apple Developer ID; with a real cert macOS would
# also accept hash drift via designated-requirement matching.
cmd_macos_stabilize_bundle_id() {
    local target="${1:-}"
    case "$(uname -s)" in
        Darwin) ;;
        *) return ;;
    esac
    case "$target" in
        ""|*-apple-darwin|universal-apple-darwin) ;;
        *) return ;;
    esac

    local bundle_root
    if [[ -n "$target" ]]; then
        bundle_root="$ROOT_DIR/target/$target/release/bundle/macos"
    else
        bundle_root="$ROOT_DIR/target/release/bundle/macos"
    fi
    if [[ ! -d "$bundle_root" ]]; then
        return
    fi

    local conf="$ROOT_DIR/app/src-tauri/tauri.conf.json"
    local identifier
    identifier="$(node -e "console.log(require('$conf').identifier)")"
    if [[ -z "$identifier" ]]; then
        echo "macos-stabilize: no identifier in $conf" >&2
        return 1
    fi

    local app
    while IFS= read -r app; do
        echo "macos-stabilize: re-sign $app with identifier=$identifier"
        codesign --force --deep --sign - --identifier "$identifier" "$app"
        codesign -dv "$app" 2>&1 | grep -E '^(Identifier|Signature)=' || true
    done < <(find "$bundle_root" -maxdepth 2 -name '*.app' -type d)
}

# Build the MCP server binary for the requested target(s) and copy it
# into `app/src-tauri/binaries/` under the Rust target triple Tauri's
# externalBin lookup expects. On macOS universal builds we lipo the two
# arch slices into one fat binary because Tauri 2 doesn't auto-combine
# external binaries the way it does the main bundle.
cmd_stage_mcp_sidecar() {
    local target="${1:-}"
    local sidecar_dir="$ROOT_DIR/app/src-tauri/binaries"
    mkdir -p "$sidecar_dir"

    if [[ -z "$target" ]]; then
        # Host build — let cargo pick the default triple.
        local host_triple
        host_triple="$(rustc -vV | sed -n 's/^host: //p')"
        local ext=""
        case "$host_triple" in *windows*) ext=".exe" ;; esac
        echo "stage-mcp-sidecar: host target $host_triple"
        cargo build --release --bin projectmind-mcp
        cp "$ROOT_DIR/target/release/projectmind-mcp${ext}" \
           "$sidecar_dir/projectmind-mcp-${host_triple}${ext}"
        return
    fi

    if [[ "$target" == "universal-apple-darwin" ]]; then
        echo "stage-mcp-sidecar: universal-apple-darwin (lipo aarch64 + x86_64)"
        for t in aarch64-apple-darwin x86_64-apple-darwin; do
            cargo build --release --bin projectmind-mcp --target "$t"
            cp "$ROOT_DIR/target/$t/release/projectmind-mcp" \
               "$sidecar_dir/projectmind-mcp-$t"
        done
        lipo -create -output "$sidecar_dir/projectmind-mcp-universal-apple-darwin" \
            "$sidecar_dir/projectmind-mcp-aarch64-apple-darwin" \
            "$sidecar_dir/projectmind-mcp-x86_64-apple-darwin"
        return
    fi

    local ext=""
    case "$target" in *windows*) ext=".exe" ;; esac
    echo "stage-mcp-sidecar: $target"
    cargo build --release --bin projectmind-mcp --target "$target"
    cp "$ROOT_DIR/target/$target/release/projectmind-mcp${ext}" \
       "$sidecar_dir/projectmind-mcp-${target}${ext}"
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
    # (bundle/dmg/, bundle/macos/, bundle/deb/, bundle/rpm/, bundle/appimage/,
    # bundle/msi/, bundle/nsis/). Pick whatever distributable formats actually got produced
    # and pack them plus LICENSE + README into one archive. This keeps the
    # workflow simple: ONE artefact per target, asset_suffix telling Mac/Linux/Win
    # apart.
    local bundles=()
    while IFS= read -r f; do
        bundles+=("$f")
    done < <(find "$bundle_dir" -type f \
        \( -name "*.dmg" -o -name "*.app.tar.gz" -o -name "*.deb" \
           -o -name "*.rpm" -o -name "*.AppImage" \
           -o -name "*.msi" -o -name "*.exe" \) \
        2>/dev/null | sort)

    if [[ ${#bundles[@]} -eq 0 ]]; then
        echo "app-package: no bundle artefacts found under $bundle_dir" >&2
        find "$bundle_dir" -maxdepth 3 -type f >&2 || true
        exit 1
    fi

    # `tar -C <dir> file` requires file as a relative path inside <dir>.
    # Build a flat archive: every bundle artefact at the archive root.
    local args=()
    for f in "${bundles[@]}"; do
        local dir
        dir="$(cd "$(dirname "$f")" && pwd)"
        args+=( -C "$dir" "$(basename "$f")" )
    done
    tar czf "$archive" "${args[@]}" -C "$ROOT_DIR" LICENSE README.md
    sha256 "$archive" > "$archive.sha256"
    local size_h
    size_h="$(du -h "$archive" | cut -f1)"
    echo "app-package: $archive ($size_h, ${#bundles[@]} bundle file(s))"
}

cmd_app_stage_updater() {
    # Copy the updater-eligible bundle (and its sibling .sig if signed) into
    # `updater-stage/` with platform-stable filenames. The publish job then
    # collects these across the matrix and builds latest.json.
    local target="${1:?target triple required}"
    local suffix="${2:?asset suffix required}"
    local bundle_dir="target/${target}/release/bundle"
    local stage="updater-stage"
    mkdir -p "$stage"

    local bundle=""
    case "$suffix" in
        macos-*)
            bundle="$(find "$bundle_dir/macos" -maxdepth 2 -name '*.app.tar.gz' | head -n1 || true)"
            ;;
        linux-*)
            bundle="$(find "$bundle_dir/appimage" -maxdepth 2 -name '*.AppImage' | head -n1 || true)"
            ;;
        windows-*)
            bundle="$(find "$bundle_dir/msi" -maxdepth 2 -name '*.msi' | head -n1 || true)"
            ;;
    esac

    if [[ -z "$bundle" ]]; then
        echo "app-stage-updater: no updater bundle for $suffix — skipping (unsigned build?)"
        return 0
    fi

    local base; base="$(basename "$bundle")"
    local ext="${base#*.}"   # e.g. "app.tar.gz" / "AppImage" / "msi"
    local out="$stage/projectmind-updater-${suffix}.${ext}"
    cp "$bundle" "$out"
    echo "app-stage-updater: $out"

    if [[ -f "${bundle}.sig" ]]; then
        cp "${bundle}.sig" "${out}.sig"
        echo "app-stage-updater: ${out}.sig"
    else
        echo "app-stage-updater: no .sig sidecar — TAURI_SIGNING_PRIVATE_KEY missing? skipping signature."
    fi
}

cmd_build_latest_json() {
    # Walk the downloaded artifacts/, collect the per-platform updater bundles
    # plus their .sig contents, and emit a Tauri-compatible latest.json.
    # Skips platforms whose .sig is missing (unsigned build) instead of
    # failing — a release that ships zero signed bundles still publishes,
    # just without an updater manifest.
    local artifacts_root="${1:?artifacts root required}"
    local tag="${2:?release tag required, e.g. v0.3.5}"
    local out="${3:-latest.json}"
    local version="${tag#v}"
    local repo_url="https://github.com/Plaintext-Gmbh/projectmind/releases/download/${tag}"

    python3 - "$artifacts_root" "$tag" "$version" "$repo_url" "$out" <<'PY'
import json, os, sys, glob
from pathlib import Path

root, tag, version, repo_url, out = sys.argv[1:6]

# Map asset_suffix -> Tauri platform identifier
PLATFORM_KEY = {
    'macos-universal': 'darwin-aarch64',  # also reported under x86_64 below
    'linux-x86_64':    'linux-x86_64',
    'windows-x86_64':  'windows-x86_64',
}
# Tauri keys both Mac arches off the same universal bundle.
EXTRA_AS = {'macos-universal': ['darwin-x86_64']}

platforms = {}
for d in sorted(Path(root).glob('projectmind-updater-*')):
    suffix = d.name.removeprefix('projectmind-updater-')
    base = PLATFORM_KEY.get(suffix)
    if not base:
        continue
    bundle = next((f for f in d.iterdir() if f.is_file() and not f.name.endswith('.sig')), None)
    sig = next((f for f in d.iterdir() if f.name.endswith('.sig')), None)
    if not bundle or not sig:
        print(f'skipping {suffix}: bundle={bundle} sig={sig}', file=sys.stderr)
        continue
    entry = {
        'signature': sig.read_text(encoding='utf-8').strip(),
        'url': f'{repo_url}/{bundle.name}',
    }
    platforms[base] = entry
    for extra in EXTRA_AS.get(suffix, []):
        platforms[extra] = entry

if not platforms:
    print('build-latest-json: no signed bundles found — skipping latest.json', file=sys.stderr)
    sys.exit(0)

manifest = {
    'version': version,
    'notes': f'See https://github.com/Plaintext-Gmbh/projectmind/releases/tag/{tag}',
    'pub_date': os.environ.get('GITHUB_RUN_TIMESTAMP', ''),
    'platforms': platforms,
}
Path(out).write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
print(f'build-latest-json: wrote {out} with {len(platforms)} platforms')
PY
}

case "${1:-}" in
    check)             shift; cmd_check "$@" ;;
    test)              shift; cmd_test "$@" ;;
    install-deps)      shift; cmd_install_deps "$@" ;;
    release-build)     shift; cmd_release_build "$@" ;;
    release-smoke)     shift; cmd_release_smoke "$@" ;;
    release-package)   shift; cmd_release_package "$@" ;;
    stage-mcp-sidecar) shift; cmd_stage_mcp_sidecar "$@" ;;
    app-build)         shift; cmd_app_build "$@" ;;
    app-package)       shift; cmd_app_package "$@" ;;
    app-stage-updater) shift; cmd_app_stage_updater "$@" ;;
    build-latest-json) shift; cmd_build_latest_json "$@" ;;
    all)               cmd_check; cmd_test ;;
    -h|--help|help|"") usage ;;
    *) echo "unknown command: $1" >&2; usage; exit 2 ;;
esac
