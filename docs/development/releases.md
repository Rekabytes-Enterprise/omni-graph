# Release flow

## Problem

A source push does not publish installation artifacts. The initial main commit had no release tag and no npm publication, so both advertised installation URLs returned 404. Distribution now uses GitHub Releases for both the native archives and the npx package; an npm registry account is not required.

## Maintainer usage

```sh
# On dev, before committing a release change:
npm run version:bump -- patch
npm ci --prefix sdk
npm test
npm run check
# Commit and push dev; wait for Verify to pass.
# Promote that reviewed commit to main, then:
git tag v0.1.1
git push origin v0.1.1
```

Use `minor`, `major`, or an explicit higher stable version instead of `patch`. The script changes `package.json`, the package version in `Cargo.toml`, and Omni-graph's entry in `Cargo.lock`. It does not commit, push, or tag. `npm run version:check` validates agreement among all three files.

Verification runs on pushes to dev and PRs targeting dev. Release builds run for version tags after a gate verifies that the tagged commit belongs to main and the tag matches the source version. Manual assembly is allowed only from main and does not publish.

## Ownership and interfaces

`scripts/version.mjs` owns stable-version parsing, bump rules, and source-version agreement:

```text
readVersion(root) -> version string; reject inconsistent files
nextVersion(current, requested) -> strictly higher stable version
bumpVersion(root, requested) -> { previous, version }; update all three files
```

`release.yml` owns platform builds and distribution. A gate runs before native builds. It checks main ancestry and version agreement. Native jobs produce five platform archives and npm-native directories. The package job validates completeness, creates the stable `omni-graph.tgz` and `omni-graph-sdk.tgz` assets, and emits SHA256SUMS. The publish job uploads the completed artifact set to a GitHub release for the already existing tag.

Consumers use the native curl installer or `npx --package=https://github.com/Rekabytes-Enterprise/omni-graph/releases/latest/download/omni-graph.tgz omni-graph install`. A repository checkout is not a substitute for a release tarball because compiled native binaries are excluded from Git.

## Design decision

Two shapes were considered independently:

1. Release on every main push when its version is new. This hides tagging but needs remote release lookup, skip/retry semantics, and an implicit publication decision for every documentation push.
2. Release on explicit version tags, with a main-ancestry gate. This makes publication deliberate, reuses the existing workflow, and keeps the version script independent of network and credentials.

The second is the chosen shape. It concentrates version policy in one script and keeps publication in the workflow. Tests read the source version rather than hand-maintaining another version list. Main ancestry is checked before building so a dev-only tag cannot publish.

## Tradeoffs and scope

- Maintainers explicitly promote dev to main and push a version tag; the bump script does not perform Git operations.
- GitHub-hosted npx installation uses the release tarball URL. The short registry name remains unavailable until a separate npm publication is authorized and authenticated.
- Released assets are immutable; failed builds can be rerun before publication. Do not move an existing release tag to repair source.
- Manual assembly is useful for checking artifacts but cannot bypass the main-only release gate.


## SDK and upgrade checks

The SDK is an independent package in `sdk/`. Build and test it through the root
scripts; `npm ci --prefix sdk` installs its locked development tools. `npm pack
--prefix sdk` is not a registry publication. Publish it only when explicitly
requested, and test the packed package in a consumer directory before declaring it
ready.

Release validation must cover an existing API 1 consumer and pre-upgrade stores,
not only fresh installs. Document old updater behavior: v0.2.7 self-update needs a
one-time explicit preparation after binary replacement; the new installer and
subsequent self-updates prepare validated registered stores themselves.
