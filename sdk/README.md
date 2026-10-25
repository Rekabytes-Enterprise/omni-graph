# Omni-graph SDK

A Node.js TypeScript client for the local Omni-graph executable. The Rust engine owns SQLite, backups, migrations and graph validation. The SDK communicates through integration API 1 and never opens the database.

## Install

The SDK is distributed as a tarball on GitHub Releases. It is **not published to
the npm registry**. Install the SDK from this release:

```sh
npm install https://github.com/Rekabytes-Enterprise/omni-graph/releases/download/v0.2.8/omni-graph-sdk.tgz
```

Or build and install a tarball from source:

```sh
# From the omni-graph repository:
cd sdk
npm ci
npm pack

# From your application directory, using the actual tarball path:
npm install /absolute/path/to/omni-graph/sdk/omni-graph-sdk-0.1.0.tgz
```

The current package name is `@omni-graph/sdk`. The tarball contains compiled
JavaScript and TypeScript declarations. It does not bundle or install the Rust
executable. Install Omni-graph separately and pass its absolute executable path.
The new preparation workflow requires an engine advertising `storage_preparation`;
v0.2.8 introduces that capability; v0.2.7 does not advertise it.

Node.js 20 or newer is required at runtime. This is a server-side Node SDK, not a
browser SDK. The package exports ESM; use `import` from an ESM application. Node.js
22 or newer is needed for development tests that create SQLite fixtures.

## Quick start

Initialize the project once using your selected executable:

```sh
/path/to/omni-graph --directory /absolute/path/to/project init --agent none
```

`--agent none` avoids adding agent instructions. Omit it when you want the usual
skill setup. Then connect from your application:


```ts
import {OmniGraph, OmniGraphError} from '@omni-graph/sdk';

const memory = new OmniGraph({
  executable: '/home/user/.omni-graph/bin/omni-graph',
  directory: '/home/user/project',
  migration: 'supported',
});

await memory.connect();
const context = await memory.recall('payment retries', {limit: 8});
await memory.remember({
  title: 'Payment retries use a stable operation ID',
  type: 'lesson',
  body: 'Use the same operation ID across retries.',
}, {idempotencyKey: 'task-123-payment-lesson'});
```

`directory` must refer to an initialized Omni-graph project. Initialize a new project separately with `omni-graph init`. `initialize: true` permits preparation of missing selected stores and legacy import; it does not create project identity or install agent instructions.

## Constructor options

| Option | Default | Meaning |
| --- | --- | --- |
| `executable` | Required | Absolute path to the engine. Windows paths end in `omni-graph.exe`. Keep the stable installation path across updates. |
| `directory` | Required | Absolute path to an initialized project, or a directory beneath it. |
| `migration` | `'supported'` | Permit engine-supported SQLite migrations with backups. Use `'manual'` to return a readiness error instead. |
| `initialize` | `false` | Permit missing-store initialization and legacy import. This is separate from SQLite migration policy. |
| `env` | Inherited process environment | Override child-process environment values, such as `OMNI_GRAPH_HOME`. |
| `timeoutMs` | `30000` | Positive integer timeout for each subprocess, not the whole SDK call. |
| `maxOutputBytes` | `16777216` | Positive integer limit for combined subprocess stdout and stderr. |

Use a separate memory home for tests or explicitly isolated installations:

```ts
const isolated = new OmniGraph({
  executable: '/absolute/path/to/omni-graph',
  directory: '/absolute/path/to/test-project',
  env: { OMNI_GRAPH_HOME: '/absolute/path/to/test-memory-home' },
  initialize: true,
  timeoutMs: 120_000,
});
```

Initialize that test project with the same memory-home setting. `env` is captured
when the client is constructed; create a new client when configuration changes.

## Connection policy

Constructing a client does not run the executable. `connect`, `reconnect`, and every memory operation inspect the selected stores first. By default, calling these methods authorizes supported SQLite migrations with engine-managed backups. Set `migration: 'manual'` to prevent automatic migration. Missing stores and legacy imports additionally require `initialize: true`.

The SDK only deduplicates concurrent in-flight connection checks. It does not cache readiness between operations. Compatibility inspection currently copies selected databases for integrity checks, so this adds I/O for large stores. Replacement of the configured executable or migration by another process is detected. Keep the stable configured installation path; the SDK does not pin a symlink's historical target. It rechecks actual compatibility after preparation.

`inspect()` is always read-only. If a store is unsupported, corrupt, inaccessible or busy, the SDK refuses preparation and returns a typed error with the compatibility report in `details`. Resolve the cause and reconnect. Completed migrations in other stores remain; there is no cross-store rollback.

Existing API 1 executables remain usable for methods whose capabilities they advertise. An older executable without `storage_preparation` returns `unsupported_capability` when migration is needed. Upgrade that executable or prepare stores through its documented workflow. An SDK cannot silently repair an application adapter that never invokes it.

## API reference

All methods return promises. `connect()` is optional before a memory operation:
each operation performs the connection checks itself. There is no persistent
subprocess or socket to close, and no `disconnect()` method is needed.

| Method | Result | Behavior |
| --- | --- | --- |
| `inspect()` | `CompatibilityReport` | Read-only report, including expected non-ready statuses. |
| `connect()` | `CompatibilityReport` | Check readiness and prepare according to policy; reject if not ready. |
| `reconnect()` | `CompatibilityReport` | Run the same connection workflow against the configured path. |
| `list(filter?)` | `Memory[]` | List accessible memories. |
| `search(query, filter?)` | Ranked hits | Each hit contains `memory`, `score`, `reason`, and image metadata. |
| `recall(query, options?)` | Recall result | Contains `query`, `results`, `edges`, and `guidance`. Supports `depth`. |
| `show(id)` | Memory details | Contains `memory`, incident `edges`, and image metadata. |
| `remember(input, { idempotencyKey }?)` | `Memory` | Create a standalone or linked memory; optional images save atomically. |
| `update(id, patch)` | `Memory` | Edit mutable fields of an existing memory. |
| `link({ from, relation, to })` | `Edge` | Add a relationship between existing records. |

The SDK currently has no delete, unlink, image export, or application-update
method. Use the documented CLI for those operations. Image results contain
metadata, not image bytes.

### Filters and recall

`list`, `search`, and `recall` accept these filters:

| Field | Meaning |
| --- | --- |
| `scope` | `'global'`, `'client'`, `'project'`, or `'task'`. Existing scope permissions still apply. |
| `type` | Memory kind, such as `'issue'`, `'fix'`, `'lesson'`, or `'preference'`. |
| `status` | `'active'`, `'draft'`, `'superseded'`, or `'archived'`. |
| `task` | Task identifier, corresponding to stored `task_id`. |
| `topic` | One topic to match. |
| `limit` | Integer from 1 to 500; engine default is 30. |
| `all` | Include inactive and expired records. Defaults to `false`. |

Recall additionally accepts `depth`, an integer from 0 to 4, default 1. Scope and
task filters do not grant access to otherwise inaccessible memories.

```ts
const lessons = await memory.search('payment retry', {
  scope: 'project', type: 'lesson', limit: 10,
});
const context = await memory.recall('payment retry', {
  task: 'checkout-repair', depth: 2, limit: 8,
});
for (const hit of context.results) {
  console.log(hit.memory.id, hit.memory.summary, hit.reason);
}
```

### Save a memory and its relationships

`MemoryInput` requires `title`; other fields use engine defaults. Scope defaults
to project. Task-scoped memories also require `task_id`. Use evidence, sources,
triggers and verification fields to explain what supports a saved claim.

```ts
const issue = await memory.remember({
  title: 'Repeated payment callback created duplicate work',
  type: 'issue', scope: 'project',
  body: 'Observed two work items for one callback ID.',
  evidence_level: 'reported',
  sources: ['Issue tracker: PAY-123'],
}, { idempotencyKey: 'PAY-123-issue' });

const fix = await memory.remember({
  title: 'Deduplicate payment callbacks by callback ID',
  type: 'fix', scope: 'project',
  body: 'Claim the callback ID before creating a work item.',
  links: [{ from: issue.id, relation: 'corrected_by' }],
}, { idempotencyKey: 'PAY-123-fix' });

const details = await memory.show(fix.id);
console.log(details.edges);
```

In a creation link, `from` means existing record → new record, and `to` means
new record → existing record. Provide exactly one endpoint per link. Endpoints
must already exist. Incoming links must share the new memory's store; outgoing
links can reference accessible broader scopes under engine rules. Omit `links`
when no relationship is justified.

Supported relations are `belongs_to`, `applies_to`, `depends_on`, `supersedes`,
`derived_from`, `contradicts`, `related_to`, `corrected_by`, `teaches`, and
`resolved_by`. A `supersedes` link also retires the old record under engine rules.

### Attach a visual reference

```ts
const reference = await memory.remember({
  title: 'Approved checkout error layout',
  type: 'decision', scope: 'project',
  body: 'Keep the error beneath the payment fields.',
  images: [{
    file: '/absolute/path/to/checkout-error.png',
    caption: 'Approved reference; preserve spacing and error placement.',
  }],
}, { idempotencyKey: 'checkout-error-reference-v1' });
console.log((await memory.show(reference.id)).images);
```

The engine copies image data into the owning store. Relative image paths resolve
from the client's `directory`; absolute paths make the intended file clearer.
Image and link validation failures leave no partial creation. For retries, keep
source files readable and unchanged, even though the first save copied them.

### Edit or connect existing memories

```ts
await memory.update(fix.id, {
  progress: 'verified',
  evidence_level: 'verified',
  verification: ['Duplicate-callback regression test passed.'],
});
await memory.link({ from: fix.id, relation: 'related_to', to: issue.id });
```

Only record verification after performing it. IDs, ownership, scope and creation
history cannot be changed through a patch. A later `link` is a separate operation;
if it fails, retain the memory ID and repair the link instead of recreating memory.

## Retry behavior

Every write is attempted once. The SDK does not retry timeouts, interrupted processes, invalid responses or other ambiguous outcomes. For `remember`, generate and persist one idempotency key per intended save in the application, then pass the same key and payload on a retry. Changed payloads conflict. Preserve readable, unchanged image source files for keyed image retries. Calls without a key remain supported on older engines, but require inspecting current state after a lost response.

`remember` supports atomic `links` and `images` in its input. Omit links when a standalone memory is appropriate. `update` and `link` preserve existing CLI behavior and are never automatically retried.

## Errors and subprocess limits

`OmniGraphError` exposes `code`, `recovery`, `exitCode`, and optional `details`. Engine API errors retain their structured code. SDK errors include `connection_not_ready`, `preparation_failed`, `unsupported_capability`, `invalid_response`, `executable_unavailable`, `timeout`, and `output_limit`.

The SDK invokes one explicit absolute executable without a shell. JSON writes go to stdin. It inherits environment variables, with caller-supplied `env` overrides. Defaults are a 30-second subprocess timeout and 16 MiB combined stdout/stderr limit. Large migrations may need a longer timeout. Killing the subprocess does not prove a write failed; use the same retained creation key or inspect state.

## Handle errors in an application

```ts
try {
  await memory.connect();
} catch (error: unknown) {
  if (!(error instanceof OmniGraphError)) throw error;
  console.error(error.code, error.message, error.recovery);
  // Map the code to your UI. Details may contain local store paths;
  // keep diagnostic details in an appropriate local/admin view.
}
```

| Code | Suggested response |
| --- | --- |
| `connection_not_ready` | Inspect the store report in `details`; offer preparation only under the user's selected policy. |
| `preparation_failed` | Show incomplete upgrade state; resolve the failure and reconnect. Earlier store migrations can remain committed. |
| `unsupported_capability` | Disable the unavailable feature or ask the installation owner to update the engine. |
| `unsupported_schema` / `schema_changed` | Use a compatible engine; never recreate or downgrade the store. |
| `idempotency_conflict` | Reuse the original payload, or choose a new key only for an intentionally different save. |
| `timeout` / `interrupted` | Treat write completion as unknown; inspect state or repeat a keyed creation with its original inputs. |
| `invalid_response` / `output_limit` | Check the executable or query limits. Do not assume a failed response means a write was rolled back. |
| `executable_unavailable` | Check the configured path and execution permissions. |
| `invalid_options` / `invalid_input` | Correct the supplied client configuration or input. |

`details` has type `unknown`; narrow and validate it before reading it. Error codes
may be added over time, so include a fallback for unrecognized codes. Do not display
the same full diagnostic in every panel or label all readiness failures as a broken
library. For example, `migration_required` is a store status within a readiness
report; the SDK usually surfaces it through `connection_not_ready` under manual
policy, rather than as a standalone exception code.

## Move an existing application to the SDK

1. Keep the existing executable path, project directory, and memory home. Reuse the
   current stores; do not export/import them just to adopt the SDK.
2. Replace hand-built subprocess calls with the corresponding SDK methods. Use
   `migration: 'manual'` initially if your application requires explicit approval.
3. Save the user's upgrade policy in the application. Under `'supported'`, the SDK
   can prepare supported migrations with backups. Keep `initialize` false unless
   missing-store creation or legacy import is intended.
4. Retain operation keys and input payloads in the application's task/job records.
   A fresh random key on every retry defeats retry safety.
5. After an engine update, use `reconnect()` or perform a normal SDK operation.
   Both check the actual executable again. Restart long-running old engine
   processes; their writer fences remain active after migration.

Read-only `inspect()` can report `migration_required` without throwing. Decide how
to handle that status through the connection policy; do not repeatedly call
inspection expecting it to perform a migration.

Existing apps that do not adopt the SDK benefit from the new install/update flow
for validated registered stores. Unregistered or unavailable projects still need
explicit preparation. The already-released v0.2.7 self-updater only replaces the
binary. After using it to install an engine with the new preparation command, run:

```sh
omni-graph prepare --registered --initialize --api-version 1 --json
```

The new curl/npx installer performs preparation itself. Subsequent self-updates
also include preparation. SDK installation alone neither updates the engine nor
migrates databases.

## Development

```sh
npm ci
npm run generate
npm run check
OMNI_GRAPH_SDK_BINARY=/absolute/path/to/omni-graph npm test
```

Input enums and fields are generated from the Rust model. Output parsers validate fields the SDK exposes and tolerate additive API fields. Integration tests exercise migration, reconnection, retry receipts and unchanged API 1 operations using disposable stores. Fake-process failure tests currently run on Unix; real-engine integration tests run on all supported CI platforms.
