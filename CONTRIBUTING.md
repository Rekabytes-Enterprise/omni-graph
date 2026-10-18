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

Keep the versions in `Cargo.toml` and `package.json` aligned. The release workflow builds macOS x64/arm64, Linux x64/arm64, and Windows x64 executables. It assembles the npm package, native archives, the shell installer, and `SHA256SUMS`.

A `v*` tag triggers creation of a GitHub release with those files. A manual workflow run assembles reviewable artifacts without publishing a GitHub release. Run the verification workflow and review the artifacts before tagging an approved release commit.

The release workflow does **not** publish to npm. A maintainer with package ownership must review and publish the assembled npm tarball separately. The public npx command and curl command are available only after their respective first publications. Do not claim a release is available before verifying both installation paths.

## License

By submitting a contribution, you agree that it can be distributed under the repository's [MIT license](LICENSE).
