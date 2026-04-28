# Contributing to plaintext-ide

Thanks for your interest! `plaintext-ide` is in **early planning phase** — no code exists yet, so the most valuable contributions right now are **design discussions, ideas, and feedback**.

## How to Contribute

### Discuss the Design

The current design notes live under [`docs/plan/`](docs/plan/). If something is unclear, missing, or could be improved:

- Open a [Discussion](https://github.com/daniel-marthaler/plaintext-ide/discussions) for open-ended topics
- Open an [Issue](https://github.com/daniel-marthaler/plaintext-ide/issues) for concrete proposals or problems

### Reporting Bugs *(once code exists)*

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md). Include reproduction steps, expected and actual behaviour, and your environment.

### Suggesting Features

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md). Describe the use case and why it would be valuable.

### Pull Requests

1. Fork the repository
2. Create a feature branch from `master`: `git checkout -b feature/my-feature`
3. Make your changes
4. Ensure the project builds (commands TBD once the build system is in place)
5. Commit with a descriptive message
6. Push to your fork and open a Pull Request using the [PR template](.github/pull_request_template.md)

### Branch Protection

- Direct pushes to `master` are restricted to maintainers
- All contributions go through Pull Requests
- Contributors are welcome to create branches and open PRs freely

## Plugin Development

Once the plugin API is stable, third-party plugins can be developed and dropped into the `plugins/` directory at runtime. A dedicated guide will live under `docs/plugins/` once Phase 1 lands.

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## License

By contributing, you agree that your contributions will be licensed under the [MPL 2.0](LICENSE) license. New source files should include the standard MPL 2.0 header.
