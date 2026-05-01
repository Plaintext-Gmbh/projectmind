# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-05-01

First release after the rebrand from `plaintext-ide` to **ProjectMind**
and the public-repo switch. The headline is the green CI / release
pipeline; the next minor release will pick up the UI feature work that
was sitting in unpublished branches.

### Added

- **Auto-Release workflow** (`.github/workflows/auto-release.yml`):
  manual `workflow_dispatch` entry-point that bumps the version (minor
  by default, `major` opt-in), opens a `release/vX.Y.Z` PR, waits for
  CI green, squash-merges, tags the merge commit, and dispatches
  `release.yml` so the binaries get built + published in one shot.
- **CodeQL workflow** for Rust + JavaScript/TypeScript + GitHub Actions
  with a weekly re-scan, paths-ignore for `app/dist/**` and `target/**`.
- `SECURITY.md` policy with `info@plaintext.ch` as the contact and a
  pointer to GitHub's private vulnerability reporting.

### Changed

- Repository is now **public** (was a private MPL-2.0 repo).
- `master` is **branch-protected**: PR-only, required `Rust ubuntu-22.04`
  status check, linear history, no force-pushes, no deletions.
- All GitHub Actions references in workflows are pinned to **full-length
  commit SHAs** with a trailing `# vN` comment so Dependabot keeps them
  current.
- Default `GITHUB_TOKEN` permissions on `ci.yml` / `release.yml` /
  `auto-release.yml` are scoped to `contents: read`; jobs that need
  more elevate explicitly (the release-publish job to `contents: write`,
  the auto-release job to `contents: write` + `pull-requests: write`).
- Tauri app source files (`app/src-tauri/Cargo.toml`,
  `app/src/components/ClassViewer.svelte`, etc.) and Cargo workspace
  manifests bumped to **0.2.0**.
- README rewritten around the **MCP** angle — the server speaks MCP,
  so any frontier-LLM client (Claude Code, ChatGPT, Gemini CLI,
  Cursor, Continue, custom agents) can drive it. Earlier wording made
  it sound Claude-Code-specific.
- README "Status" line acknowledges the shipped Phase 1 MVP scope
  (Tauri shell, Markdown + HTML browsers, walkthrough mode,
  bidirectional MCP sync, folder-map diagram).
- `docs/SYNC.md` examples anonymised — replaced personal paths
  (`/Users/mad/codeplain/plaintext-app`) and class names with generic
  `/path/to/repo` / `com.example.UserService` placeholders.

### Fixed

- **CI**: build the SPA before clippy/test so
  `tauri::generate_context!()` finds `app/dist`. The proc macro had
  been panicking on every PR push since the project was renamed.
- **CI**: `mktemp -t projectmind-smoke` template now ends in `.XXXXXX`
  so it works on the Linux release-smoke job (BSD `mktemp` was lenient,
  GNU `mktemp` rejected the un-suffixed template).
- **ClassViewer**: shift+wheel actually scales the source code now —
  `.source` switched from `font-size: 12.5px` to `0.78em` so the
  `.root` em-scaling propagates.
- **Maven POM reader**: ported off `BytesText::unescape()` (removed in
  quick-xml 0.39) to `decode()` + `quick_xml::escape::unescape()`.
- **Tree-sitter parsers**: ported to the `LANGUAGE: LanguageFn` API
  (tree-sitter-rust 0.24 / tree-sitter-java 0.23). The workspace
  `tree-sitter` dep moved 0.22 → 0.26 to pick up `Into<Language>` for
  `LanguageFn`.

### Security

- Enabled **Dependabot security updates**, **secret scanning** with
  push protection, and **CodeQL** code scanning.
- npm `uuid` pinned to ≥ 14 via `pnpm.overrides` to satisfy
  GHSA-w5hq-g745-h8pq (the vulnerable v3/v5/v6 algorithms in mermaid's
  transient `uuid@11`).
- npm `vite` 5 → 8 + `esbuild` security patches via the multi-package
  Dependabot security update.
- `glib` (transient via wry) and `rand` (build-time only via
  `phf_macros` → `tauri-utils`) advisories dismissed as
  `tolerable_risk` after analysis; the `cargo update` since then has
  also patched `rand` to its fixed range.

### Removed

- Stale `app/package-lock.json`. The project uses pnpm; the npm
  lockfile was a residue from the rebrand and was triggering a
  duplicate `uuid` Dependabot alert against the npm manifest.

### Cargo dependency bumps

| Crate              | From    | To       |
| ------------------ | ------- | -------- |
| `notify`           | 6.1.1   | 8.2.0    |
| `thiserror`        | 1.0.69  | 2.0.18   |
| `quick-xml`        | 0.36.2  | 0.39.2   |
| `tree-sitter`      | 0.22.6  | 0.26.8   |
| `tree-sitter-java` | 0.21.0  | 0.23.5   |
| `tree-sitter-rust` | 0.21.2  | 0.24.2   |
| `git2`             | 0.19.0  | 0.20.4   |
| `dirs`             | 5.0.1   | 6.0.0    |
| `tauri`            | 2.10.3  | 2.11.0   |

### npm dependency bumps

| Package                       | From    | To      |
| ----------------------------- | ------- | ------- |
| `vite`                        | 5.4.21  | 8.0.10  |
| `@sveltejs/vite-plugin-svelte`| 5.x     | 7.0.0   |
| `esbuild`                     | (sec.)  | latest  |

## [0.1.0] — 2026-04-29

> **Note:** v0.1.0 was published under the pre-rebrand
> `plaintext-ide-mcp-*` asset name and only included a single macOS arm64
> tarball. It is superseded by 0.2.0; the install script and Auto-Release
> workflow target the 0.2+ asset naming.

### Added

- Initial repository scaffolding
- Vision and brainstorming document under `docs/plan/`
- MPL 2.0 license, Code of Conduct, contribution guidelines
- Issue and pull request templates
- Dependabot configuration

[Unreleased]: https://github.com/Plaintext-Gmbh/projectmind/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/Plaintext-Gmbh/projectmind/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Plaintext-Gmbh/projectmind/releases/tag/v0.1.0
