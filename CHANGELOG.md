# Changelog

## 0.2.0

- Choose Web UI or TUI when running `omni-graph`; direct `web` and `tui` commands bypass the menu.
- Explore local memories in an embedded browser UI at `http://127.0.0.1:52716`, with pan, zoom, draggable 3D sphere nodes, full 360° orbit and tilt, curved animated connections, filters and details.
- Give memory types distinct colors and icons across spheres, lists, details and a clickable legend, with soft glow and a depth grid.
- Refresh agent-saved memories automatically, using the same scoped workspace snapshot as the CLI and TUI.
- Serve assets offline from the native executable with loopback-only, read-only HTTP routes.

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
