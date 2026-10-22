# Public integration contract

Omni-graph owns its SQLite storage. Applications, CLI wrappers, desktop tools,
agent frameworks, and services should invoke its public CLI with JSON output.
Direct SQL, database table names, and the browser UI's internal HTTP endpoints are
not supported integration interfaces.

This policy describes the implementation in this source tree. It does not announce
a release or promise an LTS maintenance period. Executable release versions,
integration API versions, SQLite schema versions, and interchange document versions
are separate identifiers. Never infer one from another.

## Discover before use

```sh
/path/to/omni-graph compatibility --json
/path/to/omni-graph --directory /work/project compatibility --json
/path/to/omni-graph compatibility --scope global --json
/path/to/omni-graph compatibility --store /path/to/memory-home --json
/path/to/omni-graph --api-version 1 --json compatibility
```

`--store` takes a **directory**, not a SQLite filename. It is a discovery option,
not a new selector for normal memory commands. It cannot be combined with
`--scope`. Discovery uses the same `--directory` project discovery and
`OMNI_GRAPH_HOME` user-home override as ordinary commands. With no explicit scope,
it inspects global, configured client, and project stores when an initialized
project is found, or global memory when no project is found. `--scope task` selects
the project database; tasks do not have separate databases. Explicit project or
client scope requires a valid initialized project. Global scope works independently
of project configuration.

Discovery resolves the executable actually running and reports its canonical
absolute path. Windows canonical paths may use the `\\?\` namespace prefix;
compare resolved paths rather than raw spellings. Store paths are absolute. Discovery never installs or downloads
software, creates stores, changes configuration, or migrates a database. It copies
existing database/WAL bytes into private temporary storage and inspects those
copies. It reports no memory content or credentials. It needs enough temporary disk
space for two copies of the selected database and WAL; stores are processed in turn.
Normal store opening also uses this preflight to reject future schemas before touching
source sidecars. This adds disk I/O proportional to store size, including image bytes.

A representative successful response is:

```json
{
  "executable": {"version": "0.2.6", "path": "/opt/omni-graph/bin/omni-graph"},
  "integration_api_version": 1,
  "supported_api_versions": [1],
  "database": {
    "read_versions": [3],
    "write_versions": [3],
    "migratable_versions": [1, 2],
    "migration_target": 3,
    "migration_policy": "automatic_on_open_with_backup"
  },
  "capabilities": [
    "compatibility", "structured_errors", "memory_crud", "fts5_search",
    "graph_recall", "atomic_memory_links", "image_attachments",
    "shared_export_import", "migration_backup", "writer_fencing"
  ],
  "stores": [{
    "path": "/home/user/.omni-graph",
    "detected_schema_version": 3,
    "status": "compatible",
    "migration_required": false,
    "migration_supported": false,
    "recovery": "Proceed through the supported CLI. Recheck after updating the executable; operations still validate at use time."
  }],
  "status": "compatible",
  "exit_code": 0
}
```

The executable version above reflects the development source's current package
version, not a claim that every already-published binary with that version provides
this contract. Discover capabilities from the actual executable.

For an unsupported schema, the envelope retains executable/API/database/capability
information and reports, for example:

```json
{
  "path": "/home/user/.omni-graph",
  "detected_schema_version": 99,
  "status": "unsupported_schema",
  "migration_required": false,
  "migration_supported": false,
  "recovery": "Use an executable that supports this schema. Update through the installation owner, then invoke that exact path and recheck. Do not downgrade or recreate the store."
}
```

This is a `stores` entry; the full response also has `status: "unsupported_schema"`
and `exit_code: 12` if no other selected store has a higher exit code. Unsupported
schemas are never silently treated as empty stores. `migration_supported: false`
on an already compatible store means no migration is applicable.

## Statuses and exit codes

Consumers must inspect every `stores` entry. The overall status is the status with
the highest exit code, and the process exit code equals the response's `exit_code`.
Do not interpret numeric order as a severity ranking or depend on store ordering.

| Discovery status | Exit | Action |
| --- | ---: | --- |
| `compatible` | 0 | Proceed, with operation-time checks still required. |
| `missing_store` | 10 | Initialize intentionally; discovery creates nothing. |
| `migration_required` | 11 | Stop older processes; normal opening backs up and migrates schema 1 or 2. |
| `legacy_migration_required` | 11 | Preserve `graph.json`; normal opening validates and imports it. |
| `unsupported_schema` | 12 | Select/update to a compatible executable; never downgrade the store. |
| `permission_denied` | 13 | Correct access for the invoking user. |
| `invalid_database` | 14 | Stop writers, preserve files, investigate or restore a verified backup. |
| `unrecognized_database` | 14 | Select the correct Omni-graph store. |
| `busy` | 15 | Pause writes and retry inspection. |
| `io_error` | 16 | Check storage, paths, and temporary-directory access. |

Legacy discovery detects the file's presence, not the validity of its JSON. It
therefore leaves `migration_supported` false until the normal import validates it.

## API negotiation and errors

API 1 is the supported integration major version. Use `--api-version 1 --json` on
normal CLI commands to opt into structured errors on stdout. Successful command
payloads retain their existing JSON shape; there is no new success wrapper.
Discovery emits its JSON contract even without the version flag. Diagnostics must
be handled separately on stderr. `--help` and `--version` remain human-readable
utility output.

```sh
/path/to/omni-graph --directory /work/project --api-version 1 --json recall "layout constraints"
/path/to/omni-graph --directory /work/project --api-version 1 --json remember --file memory.json
```

The repository-maintained [compatibility response schema](../contracts/compatibility-v1.schema.json)
and [structured error schema](../contracts/error-v1.schema.json) are the
authoritative wire definitions; tests validate actual CLI output against them. Consumers must ignore unknown
optional object fields, tolerate additional capability names, and check required
capabilities before use. An unavailable capability means disable that integration
feature or present an update requirement. Do not infer it from a version number.
Unknown statuses or errors should fail closed with their readable recovery guidance,
not trigger deletion, reset, or an automatic retry of a memory creation.

An unsupported API request returns exit 17 and stdout:

```json
{
  "error": {
    "integration_api_version": 1,
    "code": "unsupported_api_version",
    "message": "Integration API 99 is not supported.",
    "recovery": "Discover supported_api_versions with compatibility --json, then choose a mutually supported version.",
    "exit_code": 17
  }
}
```

`invalid_arguments` exits 2, `store_selection_failed` exits 16, and
`operation_failed`, `backup_failed`, and `migration_failed` exit 1.
`unsupported_schema` and `schema_changed` operation errors exit 12. A failed
backup prevents migration; a failed migration retains its recovery backup.
`operation_failed` is a deliberately generic fallback; arbitrary
input/database diagnostic strings can expose memory data. Use discovery for a
specific store diagnosis. Expected incompatibility is a discovery report, not an
error envelope. Commands without `--api-version` retain historical behavior:
`--json` errors are `{"error":"readable text"}` on stderr with exit 1; ordinary
errors are readable stderr. Existing consumers can adopt API 1 explicitly without
having their legacy envelope silently replaced.

Within an API major version, optional fields and capabilities may be added.
Removing/renaming required fields, incompatible type changes, or changing the
meaning of an existing contract requires a new API major version. Deprecations
will be documented in release notes and this contract before removal in a future
major; no time-based support guarantee or indefinite support is established here.

## Supported storage matrix

| On-disk format | API 1 behavior | Resulting SQLite schema |
| --- | --- | ---: |
| Missing store | Discovery reports missing; intentional initialization creates it. | 3 |
| Legacy `graph.json` schema 1 | Normal opening validates and imports; original JSON remains. | 3 |
| SQLite schema 1 | Back up, add image storage, update search storage and writer guards. | 3 |
| SQLite schema 2 | Back up, update search storage and writer guards. | 3 |
| SQLite schema 3 | Native read/write. | 3 |
| Other SQLite schema | Refuse; no downgrade or speculative future support. | Unchanged |

Schemas 1 and 2 are migration inputs, not native read/write formats for this
executable. Normal store opening, including a read command, automatically upgrades
them. Compatibility inspection never does. No separate migration command is
required. For a project, a normal `list --json` opens its accessible stores; perform
that step only once the operator has approved the migration through their own
policy. Migration is atomic per store, not across global/client/project databases.

Before migrating, Omni-graph obtains SQLite's immediate transaction writer lock,
rechecks schema identity, and creates a standalone backup using SQLite's backup
API. This includes committed WAL data; copying only a live `memory.sqlite` file is
not a safe backup procedure. The retained file is named
`memory.sqlite.backup-v<old-schema>-<unique-id>` beside the database, is checked,
and is not overwritten. Backups can contain private memories and images; retain
and protect them according to your own policy.

Migration preserves memory IDs, records, edges, images, and search semantics.
Schema 3 adds database write guards and changes the private FTS table name so old
maintenance commands also fail closed. New writers verify schema while holding the
SQLite write transaction. Already-running historical binaries cannot perform their
previous ordinary memory/edge/image writes after migration: their connections lack
the required SQL function. Old search/reindex commands may fail because private
storage changed. Restart all old processes with the selected compatible binary.
This fence is not a defense against arbitrary direct SQL clients that remove guards;
such database access is outside the public contract.

Backup failure prevents migration. Migration statement failure rolls back the
transaction while retaining the backup. A commit failure requires fresh inspection;
never assume a success message lost during process interruption means the operation
was not committed. Legacy import retains its original source file, while new SQLite
creation/import and backup-file publication are not a single filesystem transaction.
An interrupted first initialization may leave an empty SQLite file; preserve the
legacy source and investigate before retrying.

Discovery is advisory, not a reservation: two matching copies reduce concurrent
copy races but are not a filesystem snapshot. Stop writers for a decisive diagnosis.
A nonempty rollback journal produces `busy`; preserve it for SQLite recovery with
a compatible tool after stopping writers. An orphan WAL without its database is
reported as `invalid_database`, never as a missing store.
A compatible result cannot promise that another process will not migrate the store
before the next operation. SQLite/WAL stores belong on supported local filesystems,
not a network-mounted multi-host shared write path.

## Installation, updating, and recovery

Multiple copies can coexist: Cargo's bin directory, the managed memory-home `bin`,
a package manager, an application bundle, and a downloaded binary. Resolve and pin
your configured executable rather than assuming the first `omni-graph` on PATH is
the expected copy. On Unix, `type -a omni-graph` helps an operator identify PATH
copies; discovery's `executable.path` identifies the one actually invoked.

The built-in updater installs into the managed memory home's `bin` destination.
It does not promise to replace a Cargo, package-manager, or application-managed
copy used to launch it. The installation owner decides whether to use the updater,
a package manager, source rebuild, or an application deployment. Discovery never
chooses or runs an updater. The installer can open/migrate user memory; a binary
update by itself does not migrate stores.

Check compatibility before an update, perform the operator-approved update, then
invoke the exact configured path and check again. If it still reports an old
executable, correct that selection rather than changing the database. Historical
binaries may not have a `compatibility` command at all; treat that as contract
unavailable and require an installation-owner update, not evidence of a missing or
corrupt store.

After a failed update, preserve the existing executable and all stores and inspect
what executable is actually running. After a failed migration, preserve database,
WAL, and retained backup; stop every process before repair or restoration. Restore
a verified backup with the executable that supports its schema, replacing the store
as a coordinated operator operation and preventing stale WAL/SHM files from being
replayed against it. Keep the failed store separately for investigation. Restoration
loses writes made after that backup; reconcile or export them with compatible tools
first where possible. Rolling back just the executable is safe only if it supports
the current database schema. Never downgrade a database to accommodate an old binary.

## Generic consumer example

This example pins a configured absolute executable, checks API/capabilities and
all selected stores, and leaves updating/migration to the operator. It performs no
automatic install or migration. Re-run the same check after the operator completes
the displayed recovery action.

```python
import json
from pathlib import Path
import subprocess

configured = Path("/opt/omni-graph/bin/omni-graph")
if not configured.is_absolute():
    raise RuntimeError("Configure an absolute Omni-graph executable path")
executable = configured.resolve(strict=True)
project = "/work/project"

def check_compatibility():
    result = subprocess.run(
        [str(executable), "--directory", project, "compatibility", "--json"],
        capture_output=True, text=True, timeout=60, check=False,
    )
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError("Compatibility contract unavailable; check the installation owner") from exc
    if "error" in report:
        raise RuntimeError(report["error"]["recovery"])
    if 1 not in report["supported_api_versions"]:
        raise RuntimeError("This consumer requires integration API 1")
    if "graph_recall" not in report["capabilities"]:
        raise RuntimeError("This executable does not offer graph recall")
    if result.returncode != report["exit_code"]:
        raise RuntimeError("Unexpected compatibility response")
    problems = [s for s in report["stores"] if s["status"] != "compatible"]
    if result.returncode != 0 or problems:
        raise RuntimeError("\n".join(s["path"] + ": " + s["recovery"] for s in problems))
    return report

report = check_compatibility()
# Proceed with the verified path, API 1, and ordinary per-operation error handling.
# After an operator-approved update or migration, call check_compatibility() again.
```

Do not blindly retry `remember` after a lost response. Creation has no idempotency
key; inspect existing records before deciding whether it needs to be repeated.
