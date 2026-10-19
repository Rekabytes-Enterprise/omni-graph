# Contributing to Omni-graph

Thank you for helping improve Omni-graph. Bug reports, documentation, agent integrations, and focused code changes are welcome.

## Branch policy

**Create every contribution branch from `dev`. Open every contribution pull request against `dev`, never `main`.**

`main` is the stable release branch and is maintained by the maintainers. A check rejects contribution PRs targeting another branch.

1. Fork the repository and clone your fork.
2. Add the upstream repository and fetch its branches.
3. Create a feature branch from the latest upstream `dev`.

```sh
git clone https://github.com/YOUR_USERNAME/omni-graph.git
cd omni-graph
git remote add upstream https://github.com/Rekabytes-Enterprise/omni-graph.git
git fetch upstream
git switch -c feat/your-change upstream/dev
```

Replace `YOUR_USERNAME` and choose a descriptive branch name. Never start a contribution branch from `main`. If `dev` is missing, ask a maintainer to create it rather than opening a PR against `main`.

Before opening a PR, update your branch with `dev`:

```sh
git fetch upstream
git rebase upstream/dev
git push -u origin feat/your-change
```

Choose **`dev`** as the base in GitHub. With GitHub CLI, use `gh pr create --base dev`.

## Local setup

Install Rust 1.88 or newer and Node.js 20 or newer. CI uses Node.js 22. SQLite is bundled; a database server is not needed.

```sh
cargo build
```

Use an isolated memory home for manual testing so test records do not enter your real user memory:

```sh
mkdir -p /tmp/omni-graph-contribution/project
OMNI_GRAPH_HOME=/tmp/omni-graph-contribution/home \
  cargo run -- --directory /tmp/omni-graph-contribution/project init --agent none
OMNI_GRAPH_HOME=/tmp/omni-graph-contribution/home \
  cargo run -- --directory /tmp/omni-graph-contribution/project
```

The commands above are for macOS/Linux; set `OMNI_GRAPH_HOME` in your environment on Windows. Keep generated memory data, native binaries, and package tarballs out of commits. Tests create temporary data and clean it up; there is no bundled seed dataset.

## Before submitting

```sh
npm test
npm run check
```

`npm test` runs Rust unit/integration tests, the npm launcher and persistent-install smoke tests, and local shell-installer tests on macOS/Linux. Installer tests use a local download fixture and never download a release. `npm run check` runs formatting and Clippy.

For installer changes, also run `sh -n install.sh`. Exercise supported platforms where possible and explain anything you could not test.

Keep changes focused. Match existing Rust and JavaScript conventions, preserve scope boundaries and existing memories, and add meaningful tests for behavior changes. User-facing installation and usage belong in the README; development and release notes belong here or in the PR.

## Opening a pull request

Explain the problem, the resulting behavior, and how you verified it. Include terminal captures for visual changes, and describe migrations or compatibility changes when relevant.

- Base: **`dev`**.
- Head: your contribution branch, created from `dev`.
- Include relevant test results and link related issues.
- Do not commit private memory databases, client information, tokens, or generated build artifacts.

Maintainers may ask you to rebase or narrow the scope before merging. Do not merge contribution PRs into `main`.

## Reporting bugs

Open an issue with the Omni-graph version, operating system, terminal, steps to reproduce, expected behavior, and actual result. Remove private memories and credentials from examples and logs. Suggest larger features in an issue before building them.

For suspected security issues, avoid posting secrets or working exploit details in a public issue. Contact the maintainers privately through GitHub.

## Releases for maintainers

Verification CI runs only for pushes to `dev` and PRs targeting `dev`. Stable release builds use version tags, with a gate that rejects commits outside `main`. Manual artifact builds must also run from `main`.

On `dev`, use the version script before committing a release change:

```sh
npm run version:bump -- patch
# Also accepts minor, major, or an explicit higher stable version.
npm run version:check
npm test
npm run check
```

The bump command updates `package.json`, `Cargo.toml`, and Omni-graph's version in `Cargo.lock`; it never commits, pushes, or creates a tag. Do not use `npm version` alone, which leaves Rust metadata out of sync.

Push `dev` and wait for verification to pass. Maintainers then promote the reviewed commit to `main` and tag that commit:

```sh
git switch main
git merge --ff-only dev
git push origin main
git tag v0.1.1
git push origin v0.1.1
```

Use the version actually reported by `npm run version:check` for the tag. The release workflow builds five native platforms, archives, `omni-graph.tgz`, the shell installer, and `SHA256SUMS`, then publishes them to GitHub Releases. The tarball is directly usable by npx through its GitHub URL; no npm registry login or publication is needed. A source checkout alone lacks the native binaries.

The workflow does not publish to npm. If a registry release is wanted later, treat that as a separate authenticated publication. Verify both the public curl URL and the public npx tarball URL after each release. See [release design and version ownership](docs/development/releases.md) for details.

## License

By submitting a contribution, you agree that it can be distributed under the repository's [MIT license](LICENSE).
