# Atomic memory creation

## Problem and usage

Automatic writers previously called `remember`, then `link`. An interruption or
failed edge write left the memory without intended lineage. Memory data, FTS5,
edges, and supersession status already share a SQLite transaction within a store;
the missing piece was a creation request that uses that boundary.

`remember --file - --json` accepts the existing memory fields plus optional links:

```json
{
  "title": "Verified cache fix",
  "type": "fix",
  "links": [
    {"from": "mem_EXISTING_ISSUE_ID", "relation": "corrected_by"},
    {"relation": "belongs_to", "to": "mem_EXISTING_COMPONENT_ID"}
  ]
}
```

Replace example IDs with real memory IDs. `from` means existing → new;
`to` means new → existing. Each entry requires exactly one endpoint. Omitting
`links` or passing `[]` creates a standalone memory. Creation returns the usual
memory object, preserving its top-level ID contract; `show ID` reads back links.

## Shape and ownership

```text
CreateLink { relation: Relation, from: Option<String>, to: Option<String> }
Workspace::create_with_images(MemoryInput, &[CreateLink], &[ImageInput]) -> Memory
Workspace::link(from, relation, to) -> Edge
apply_link(&mut Graph, source, relation, target) -> Edge [private]
```

The CLI extracts links from the request without storing them as memory metadata.
Both creation and the existing link command use storage-owned edge validation.
The storage layer resolves only accessible endpoints, rechecks same-store records
under the write transaction, inserts the memory and edges, and commits once.
Any validation or SQL error rolls back the new memory, all edges, supersession
changes, and FTS updates. Duplicate edges coalesce as before.

All written edge sources must live in the new memory's store. Incoming links from
other stores fail, even if a standalone link would be permitted. Outgoing links to
accessible broader scopes are read-only references, as with existing links.
Supersession keeps the same-scope/task, active-successor and cycle rules.
Broader referenced databases are not part of a distributed transaction.

## Synthesis decision and alternatives

Two candidate shapes were considered sequentially: extend memory creation inside
its existing store transaction, or add a general batch API spanning memories and
stores. The first hides rollback and edge policy behind a small compatible input
extension and reuses SQLite's existing guarantees. The second exposes operation
ordering, temporary IDs, multiple locks, and cross-database recovery to callers;
it was rejected for this request. Shared edge validation avoids separate policies
for batch creation and the existing link command.

We accept a single-store write boundary in exchange for reliable local atomicity.
We retain the standalone link command for relationships discovered later; it is
not needed to finish a linked creation request. Optional retry keys now use schema-4
creation receipts in the same transaction. The receipt is rechecked under the writer
lock, so concurrent identical requests return the same ID. Key normalization and
request fingerprint version 1 are durable contracts: future MemoryInput changes
must preserve their canonical representation or support prior fingerprint versions.
See the [public retry contract](../integration-contract.md#retry-safe-creation).
