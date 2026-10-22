# Compatibility contract implementation

The public policy and consumer examples are in
[the integration contract](../integration-contract.md). This document records the
implementation boundaries and why they exist.

## Separate discovery from opening

`Store::connection` performs ordinary product behavior: setup locking, housekeeping,
initialization, and migration. It cannot implement read-only discovery.
`compatibility_probe::inspect` instead streams database and WAL into two private
temporary directories, compares digests, and retries changing sources up to three
times. SQLite inspects only a private copy, runs `quick_check`, validates the
application ID, and reads `user_version`. Source SHM files are deliberately excluded.

A read-only SQLite connection can still create or touch shared-memory sidecars in
WAL mode. SQLite `immutable=1` can ignore live WAL changes. Both are unsuitable for
the discovery promise. The chosen copying approach leaves the selected files alone
and sees committed WAL data in the private copy. Matching copies are an advisory
stability check, not an atomic filesystem snapshot. Operation-time transaction
checks remain authoritative. Under active writes, consumers should pause writers
and repeat inspection rather than treat a transient result as grounds for repair.

The probe returns typed outcomes and no raw SQLite diagnostic text, memory content,
or credentials. Discovery maps those outcomes into stable statuses and exits.
Store-selection failures are structured errors; unsupported schemas remain normal
structured discovery reports. The maximum selected-store exit determines the
aggregate response without hiding individual store outcomes.

## Separate version namespaces

The executable's package version identifies a build lineage. API 1 identifies the
public integration contract. SQLite schema 3 identifies private persistence.
Interchange JSON schema versions are separate again. Capability names let a client
check the feature it needs without guessing from package versions.

The opt-in `--api-version 1 --json` flag introduces a structured stdout error
envelope without changing legacy `--json` errors on stderr. Successful command
payloads retain their existing shape. Generic operation failures intentionally avoid
copying arbitrary error chains that could include memory content. Contract-specific
errors carry stable codes and recovery text.

## Migrate once, fence old writers

Existing migration logic remains in `database.rs`. Schema 1 gains image storage;
schemas 1 and 2 gain schema 3 writer fences and updated private FTS storage. The
schema version changes only inside the migration transaction.

A filesystem lock coordinates current setup processes, while `BEGIN IMMEDIATE`
coordinates actual SQLite writers, including historical processes that do not use
a new locking protocol. After acquiring that transaction, migration checks the
schema again. A separate SQLite reader creates a backup using SQLite's backup API
while the write reservation prevents intervening commits. The backup includes
committed WAL frames, is checked, made independent of WAL sidecars, synchronized,
and published under a unique name before migration statements run.

Every supported write entry point must use the checked write-transaction helper.
Checking only when a connection opens leaves a race: another executable can migrate
a store before that connection starts its next write. Tests must exercise retained
connections, not only freshly opened stores.

New checks cannot change historical binaries. Schema 3 therefore installs database
triggers requiring a connection function unavailable to those binaries for ordinary
memory, edge, and image writes. Changing the private FTS table name also prevents
historical direct reindex/check SQL from modifying it after migration. The fences
cover supported historical operations, not hostile clients able to drop triggers
or edit arbitrary database files. Direct database integration remains unsupported.

Migration is atomic per database. Backup creation and migration are intentionally
separate: a failed migration retains its recovery backup. Global/client/project
stores cannot migrate as one transaction. Legacy JSON remains its own preserved
recovery source; initial filesystem creation and legacy import are not globally
atomic. There is no automatic downgrade.

## Verification boundaries

Maintain fixtures for supported SQLite schemas 1 and 2, including records,
relationships, metadata, and schema-2 images. Verify IDs and contents survive,
search still works, and backups contain committed WAL data. Test migration failure
and rollback, a writer held before migration, a retained old connection after
migration, and rejection of unsupported schemas.

Discovery tests compare source file lists and bytes before and after probing missing,
future-schema, corrupt, foreign, permission-denied, and live-WAL stores. A filesystem
permission test must account for privileged environments that bypass mode bits.
Wire tests must invoke the real CLI, validate actual JSON against maintained schemas,
assert stdout/stderr separation and exit values, and cover unsupported API requests.

Do not equate successful compilation, a tag, or a release upload with passing these
checks. Report the actual commands run and their results in each handoff. This
source-level contract does not establish compatibility with untested future schemas,
arbitrary old executable releases, or an LTS support period.
