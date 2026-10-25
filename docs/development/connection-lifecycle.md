# Connection and upgrade lifecycle

## Problem

An existing API 1 consumer may check `compatibility` before every operation and
reject `migration_required`. Updating only the executable leaves that consumer
blocked even though the new engine knows how to migrate its stores. A new SDK
cannot repair a previously shipped adapter that never invokes it.

## Ownership

The Rust engine owns store selection, validated backups, migration, writer fencing,
and preparation outcomes. Compatibility inspection remains read-only. The SDK owns
subprocess transport, API negotiation, connection policy and typed responses. It
never opens SQLite and does not download or replace an executable.

## Alternatives and decision

One alternative was to make compatibility inspection migrate implicitly. That would
change API 1's read-only promise and turn a health check into a write operation.
Another was to keep migration entirely inside a new SDK. That would leave existing
adapters blocked after an engine update.

Use explicit engine preparation, integrate it into install/update, and let the SDK
invoke it according to its connection policy. This keeps one migration owner while
covering both existing registered consumers and newly integrated consumers.

## Usage and boundaries

`prepare --json --api-version 1` migrates supported selected stores. Initialization
of missing stores and legacy import require `--initialize`. `--registered` is used
by installation/update to include validated registered projects and their client
stores. Stale registrations do not authorize creating replacement projects.

Preparation inspects the whole selection before writing. Unsupported, corrupt or
inaccessible stores block the selection. A later failure can leave earlier stores
successfully migrated: transactions and backups are per store, not cross-store.
The result records each outcome and current compatibility. Repeating preparation
finishes remaining work without recreating memory records.

After replacing an executable, the updater invokes the new executable for
preparation. The old process must not attempt to interpret the new schema itself.
Successful preparation allows existing API 1 adapters to pass their unchanged
compatibility checks. Unknown or unregistered stores still require explicit
preparation or a consumer with a supported migration policy. Long-running old
engine processes must restart; their write fences remain in effect.

A SDK connection retains the configured executable path, including an intentional
symlink, so updates at that path are discovered. It rechecks compatibility before
operations instead of trusting a version cached for the life of an application.
Writes are not blindly replayed after ambiguous process failure. A memory creation
retry retains the caller's operation key, payload and image inputs.

## Release evidence

Test old schema stores and an unchanged API 1 consumer through install and repeated
preparation. Also test mixed unsupported stores, interrupted migrations, preserved
IDs and links, executable replacement, SDK protocol errors and concurrent connects.
Fresh-install tests alone do not establish upgrade compatibility. Package version,
API version and private database schema remain independent.


## First transition from an older self-updater

Released v0.2.7 only replaces the binary during self-update. It cannot run a hook
introduced in its replacement. That first transition therefore needs explicit
`prepare --registered --initialize`, or installation through the new curl/npx
installer. Subsequent updates use the new lifecycle. Do not claim that a new SDK or
updater changes behavior already shipped in an old executable.

Installation first inspects all registered stores before replacing the executable.
Known blockers preserve the old executable. After that preflight, replacement
happens before migration so an interrupted multi-store migration retains an engine
capable of resuming it. Installation reports binary installation and storage
readiness separately. A later failure never implies that earlier migrations were
rolled back.
