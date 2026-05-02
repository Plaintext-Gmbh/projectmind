# Contributing to projectmind

Thanks for your interest! `projectmind` is an **early MVP** — the MCP server and Tauri shell both work end-to-end, but the project is still small and easy to read in one sitting. **Design discussions, bug reports and PRs are all welcome.**

## How to Contribute

### Discuss the Design

Architecture and reference docs live under [`docs/`](docs/) (see `architecture.md`, `SYNC.md`, `branding.md`). The roadmap and feature backlog live on GitHub:

- **[Discussions](https://github.com/Plaintext-Gmbh/projectmind/discussions)** for vision, brainstorming, sketches — start with the [Vision & Roadmap thread](https://github.com/Plaintext-Gmbh/projectmind/discussions/58)
- **[Issues](https://github.com/Plaintext-Gmbh/projectmind/issues)** for concrete proposals, bugs, sub-tasks of epics
- **[Project board](https://github.com/Plaintext-Gmbh/projectmind/projects)** for what's currently in flight

### Reporting Bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md). Include reproduction steps, expected and actual behaviour, and your environment.

### Suggesting Features

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md). Describe the use case and why it would be valuable.

### Pull Requests

1. Fork the repository
2. Create a feature branch from `master`: `git checkout -b feature/my-feature`
3. Make your changes
4. Run the same checks CI runs: `./scripts/ci.sh all` (fmt + clippy + tests + doctests)
5. Commit with a descriptive message
6. Push to your fork and open a Pull Request using the [PR template](.github/pull_request_template.md)

### Branch Protection

- Direct pushes to `master` are restricted to maintainers
- All contributions go through Pull Requests
- Contributors are welcome to create branches and open PRs freely

## Plugin Development

Phase 1 plugins are statically registered. Each plugin is its own crate under `plugins/` and implements either `LanguagePlugin` or `FrameworkPlugin` from `projectmind-plugin-api`. Look at `plugins/lang-rust` for a small, self-contained example. Phase 2 will add dynamic loading from a `./plugins/` directory next to the binary.

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## License

By contributing, you agree that your contributions will be licensed under the [MPL 2.0](LICENSE) license. New source files should include the standard MPL 2.0 header.
