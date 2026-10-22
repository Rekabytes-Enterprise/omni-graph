# Changelog

## 0.2.6

- Add read-only compatibility discovery, API 1 negotiation, structured errors, JSON schemas, and consumer-neutral integration guidance.
- Upgrade SQLite schemas 1 and 2 to schema 3 with verified WAL-safe backups and transactional migration.
- Fence writes from historical open connections and recheck schema inside write transactions.
- Preserve legacy CLI error output unless consumers opt into the versioned contract.

## 0.2.5

- Store scoped image attachments with memories, including atomic creation with links, Web UI previews, image CLI commands, and metadata in search/recall.
- Add visual-reference guidance for agents and include images in explicitly shared export/import bundles.
- Migrate SQLite storage from schema 1 to 2 while preserving existing memories; older executables cannot open the upgraded stores.

## 0.2.4

- Allow `remember --file` to save a memory and optional incoming/outgoing links in one transaction, with rollback of edges, supersession changes, and search entries on failure.
- Keep standalone memory creation unchanged when links are absent or empty; share scope and relationship validation with the existing link command.

## 0.2.3

- Improve the memory skill with capture-and-connect, evidence-lineage, and graph-maintenance guides; verify relationships without forcing standalone records into the graph.
- Install and refresh supporting skill guides alongside the entrypoint for supported agents, with destination checks before writing files.
- Add issue forms, an issue-closing PR template, security and support policies, a community code of conduct, and maintainer/code-owner documentation.

## 0.2.2

- Add Remote access to the startup menu and `web --remote --email EMAIL` for headless servers.
- Generate temporary HTTPS links protected by Cloudflare email/PIN sign-in, with a pinned checksum-verified helper downloaded automatically.
- Keep the origin server on loopback and accept only the exact generated tunnel origin.
- Stop and reap the tunnel on Ctrl+C, termination, SSH disconnect, startup failure or helper exit; keep existing Cloudflare configuration isolated.

## 0.2.1

- Make bare `omni-graph update` update the application from GitHub Releases, including outside a project directory.
- Preserve `update MEMORY_ID --file patch.json` for memory edits; reject incomplete memory-edit arguments.
- Verify checksums, archive entries and executable versions before replacing an installation, with Windows-aware self replacement.
- Recognize an already-active PATH during installation and clarify that existing projects do not need reinitialization.

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
