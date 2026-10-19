# Application updates

Bare `omni-graph update` installs the latest stable GitHub release into the configured memory home's `bin`, without opening a project. `update ID --file PATCH` retains its original memory-edit behavior. Clap requires ID and file together, so incomplete edits fail before any network request.

We considered delegating to the shell installer. That would require shell/curl tooling, complicate Windows and execute a downloaded script. The selected native updater uses ureq with HTTPS-only requests, bounded downloads and timeouts; constructs download URLs from a validated stable tag; verifies SHA-256 against the published manifest; and extracts exactly one regular executable. It executes `--version` to check the candidate before binary-only replacement. It never reads or changes SQLite, registries, skills or project files.

The installer and updater share one replacement routine and installation lock. Normal replacement stages and atomically renames a file. When replacing the currently running installation, self-replace handles Unix and Windows executable semantics. Cargo/npx invocations still update the persistent memory-home installation and print its full destination. Version comparisons are numeric and refuse downgrades. Reinstall remains the bootstrap path for versions before 0.2.1.

Tests cover CLI dispatch, invalid partial edits, numeric versions, checksum/entry/version rejection, successful installation and preservation of existing data. Public release verification additionally runs application update outside an initialized project. No custom release URL environment override is exposed.
