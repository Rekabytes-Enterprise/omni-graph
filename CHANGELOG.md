# Changelog

## 0.1.2

- Automatically save the installed bin directory in supported Unix shell startup files for future terminals.
- Clearly show the activation command required by the current terminal and a direct executable fallback.
- Preserve existing shell configuration, dotfile symlinks and permissions; avoid duplicate PATH entries on reinstall.
- Verify command discovery in a fresh shell for curl installs and test current-shell activation for npx installs.

## 0.1.1

- Publish native executables, the curl installer, checksums, and a directly installable npx tarball through GitHub Releases.
- Use the GitHub tarball URL for npx installation without requiring npm registry publication.
- Run verification CI only on dev pushes and pull requests targeting dev.
- Reject release tags whose commits are not on main, and reject mismatched source versions.
- Add version:bump and version:check commands that keep npm, Cargo, and the Cargo lockfile aligned.

## 0.1.0

Initial source version: local SQLite agent memory, scoped recall and learning records, agent skills, interactive terminal graph, and persistent user installation. This version was not published as a binary release.
