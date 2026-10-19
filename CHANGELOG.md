# Changelog

## 0.1.1

- Publish native executables, the curl installer, checksums, and a directly installable npx tarball through GitHub Releases.
- Use the GitHub tarball URL for npx installation without requiring npm registry publication.
- Run verification CI only on dev pushes and pull requests targeting dev.
- Reject release tags whose commits are not on main, and reject mismatched source versions.
- Add version:bump and version:check commands that keep npm, Cargo, and the Cargo lockfile aligned.

## 0.1.0

Initial source version: local SQLite agent memory, scoped recall and learning records, agent skills, interactive terminal graph, and persistent user installation. This version was not published as a binary release.
