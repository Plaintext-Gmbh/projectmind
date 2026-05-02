# Contributing to ProjectMind

Thanks for your interest! ProjectMind has shipped a Phase 1 MVP — the MCP
server and Tauri shell both work end-to-end with bidirectional MCP sync,
multi-language UI (DE / EN / FR / IT / ES), browser-style navigation
history, draw.io rendering, and a desktop-app release pipeline that
publishes binaries for Linux, macOS, and Windows. The project is still
small and easy to read in one sitting; **design discussions, bug
reports, and PRs are all welcome.**

## How to Contribute

### Discuss the Design

Architecture and reference docs live under [`docs/`](docs/) (see
`architecture.md`, `SYNC.md`, `branding.md`). The roadmap and feature
backlog live on GitHub:

- **[Discussions](https://github.com/Plaintext-Gmbh/projectmind/discussions)**
  for vision, brainstorming, sketches — start with the
  [Vision & Roadmap thread](https://github.com/Plaintext-Gmbh/projectmind/discussions/58).
- **[Issues](https://github.com/Plaintext-Gmbh/projectmind/issues)** for
  concrete proposals, bugs, sub-tasks of epics.
- **[Project board](https://github.com/Plaintext-Gmbh/projectmind/projects)**
  for what's currently in flight.

### Reporting Bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md).
Include reproduction steps, expected and actual behaviour, and your
environment.

### Suggesting Features

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md).
Describe the use case and why it would be valuable.

## Local Development

```bash
git clone git@github.com:Plaintext-Gmbh/projectmind.git
cd projectmind

# Run the same checks CI runs:
./scripts/ci.sh check        # cargo fmt --check + clippy
./scripts/ci.sh test         # cargo test (workspace + doctests)
./scripts/ci.sh all          # everything above

# Run the desktop app in dev (Tauri shell + Vite hot reload):
./build dev

# Build the desktop app bundle for the host platform:
./build app

# Frontend unit tests (vitest, runs in CI on the Linux job):
cd app && pnpm install --frozen-lockfile && pnpm test
```

`./build help` lists every supported subcommand. Releases are produced
exclusively by the GitHub Auto-Release workflow — there is no
`./build release` on purpose.

## Pull Requests

1. Fork the repository.
2. Create a feature branch from `master`:
   `git checkout -b feature/my-feature`.
3. Make your changes — small, focused PRs land faster than sweeping
   refactors.
4. Run `./scripts/ci.sh all` and `cd app && pnpm test` locally before
   pushing — CI will run the same checks plus a Tauri build per
   platform.
5. Commit with a [Conventional Commit](https://www.conventionalcommits.org/)
   prefix (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`,
   `perf:`, `build:`, `ci:`). The Auto-Release workflow respects
   semver-style bumps.
6. Push to your fork and open a Pull Request using the
   [PR template](.github/pull_request_template.md).

### Branch Protection

- Direct pushes to `master` are restricted to maintainers.
- Every contribution goes through a Pull Request.
- The required status check is **CI / Rust ubuntu-22.04**. CodeQL +
  macOS runs are advisory.
- Linear history only — squash or rebase merges, no merge commits.
- Force-pushes and branch deletions on `master` are blocked.

### Releases

ProjectMind ships via the **Release** workflow under the Actions tab:

- **`workflow_dispatch` with `bump: patch | minor | major`** — opens a
  `release/vX.Y.Z` PR that bumps every version reference. Merge it,
  push the matching tag (`git tag vX.Y.Z origin/master && git push
  origin vX.Y.Z`), and the same workflow then builds the MCP server +
  Tauri app bundles and publishes a GitHub Release.
- **`push` of a `vX.Y.Z` tag** — only the build + publish path runs.
  Useful when you've already bumped versions in a regular PR.

Patch releases for individual bug fixes are fine; please pick `minor`
or `major` when shipping a coherent set of features.

## Plugin Development

Plugins live under [`plugins/`](plugins/) and implement either
`LanguagePlugin` or `FrameworkPlugin` from
[`projectmind-plugin-api`](crates/plugin-api). Look at
[`plugins/lang-rust`](plugins/lang-rust) for a small, self-contained
example. The plugin API also exposes a `TabContribution` shape so a
plugin can light up a new tab in the desktop shell.

Plugins are statically registered today. Dynamic plugin loading from a
`./plugins/` directory next to the binary is on the Phase 2 roadmap —
see the
[plugin issues](https://github.com/Plaintext-Gmbh/projectmind/issues?q=is%3Aissue+label%3Aarea%3Aplugins).

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md).
By participating, you are expected to uphold it.

## Security

Security issues — please **don't** open a public issue. Mail
[info@plaintext.ch](mailto:info@plaintext.ch) instead. See
[`SECURITY.md`](SECURITY.md) for the full policy.

## License

By contributing, you agree that your contributions will be licensed
under the [MPL 2.0](LICENSE) license. New source files should include
the standard MPL 2.0 header (look at any existing crate for the
boilerplate).
