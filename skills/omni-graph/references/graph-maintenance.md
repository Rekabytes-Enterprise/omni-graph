# Graph maintenance

Use for a requested graph audit, suspected missing links, stale knowledge, or
duplicate evidence. A normal memory write does not require a whole-graph audit.

## Inspect before repairing

- Run `review --json` for due records, unresolved work, weak lessons, and repeated
  causes. Review does not currently report isolated nodes or repair anything.
- Run `graph --all --json` to inspect the accessible graph. A centered graph or
  filtered UI can hide neighbors; absence from that view is not proof of isolation.
- Count isolated records by checking both incoming and outgoing edge endpoints
  against the returned memories. State which scopes/statuses the snapshot covers;
  do not claim it includes inaccessible client/project stores.
- Read candidate records and underlying sources. Shared topics, source paths, and
  similar titles identify candidates for inspection, not proof of a relationship.

## Repair conservatively

Classify each candidate as a supported missing relationship, legitimate standalone
record, possible duplicate, outdated claim, or unresolved question. Apply the
directions in [capture and connect](capture-and-connect.md). Prefer a few accurate
edges over making every record connected.

Keep distinct evidence, fixes, lessons, and goals separate. Connect a report to its
source with `derived_from` rather than treating both as independent confirmations.
Check conditions before declaring contradictions. Do not turn a chronological
study outline into causal or prerequisite relationships without evidence.

For duplicate or outdated claims, inspect downstream references before updating
or superseding. `supersedes` retires the target; `unlink` does not reactivate it.
Do not archive unresolved findings merely to simplify the visual graph. Respect
scope boundaries and the user's authorized maintenance scope.

## Verify and report

Read back changed records/edges with `show` and inspect a fresh graph. Report the
specific relationships added or corrected, their source basis, remaining isolated
records, and uncertain candidates left untouched. If useful, record a reusable
lesson about the writer gap and connect it to the relevant operational context.
Avoid saving the whole audit transcript or implying experimental results changed
when only graph metadata changed.

If an automatic writer repeatedly omits links, report the integration gap. A skill
cannot enforce writes made outside its workflow. Writers need explicit parent or
evidence IDs and validation of relation direction/scope. Prefer the optional
`links` array in `remember --file` for new records so the memory and supplied
relationships commit together. This does not infer missing relationships or make
separate `update`/`link` calls atomic. Existing graph repairs still use those
commands and must verify each result.
