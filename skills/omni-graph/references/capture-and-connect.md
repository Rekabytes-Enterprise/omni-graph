# Capture and connect

Use when saving a finding, decision, fix, lesson, goal, or handoff.

## Find the context

Run `recall "<task or finding>" --json` and inspect relevant records with
`show <id> --json`. Use `search` for a known title or identifier when useful.
Read the underlying source before calling a result verified. Do not treat a
retrieved memory as independent proof of its own claim.

Update an existing record when correcting or extending the same claim. Create a
separate record when the claim, role, test conditions, or lifecycle differs:
a measurement, attempted fix, reusable lesson, and unresolved goal usually need
distinct records. Keep source paths/sections, conditions, and limitations precise
enough for another agent to check them.

## Save, connect, verify

1. Save with `remember --file - --json`, or patch with
   `update <id> --file - --json`. Capture the returned ID.
2. Inspect existing edges. Choose relevant endpoints and a supported relation
   from the table below; there is no required edge count.
3. Run `link <from-id> <relation> <to-id> --json` for each justified relationship.
4. Run `show <id> --json` and confirm both the record and intended edge directions.
   If a link fails, report which one and why. Reuse the saved ID on retry. Exact
   duplicate links are idempotent; creating the same memory again is not.

These commands are separate writes, not a transaction. Do not delete a valid
memory merely because a subsequent link failed. If no supported relation exists,
leave it standalone and say so in the task report when relevant; do not create a
new memory merely to explain an isolated node.

## Relationship direction

Only these relation names are currently supported:

| From → relation → to | Use when |
| --- | --- |
| Record → `belongs_to` → component/study | There is an explicit organizational parent. |
| Lesson/constraint → `applies_to` → component/context | The source establishes where guidance applies. |
| Work item → `depends_on` → prerequisite | Completing it requires that prerequisite; sequence alone is insufficient. |
| Report/conclusion → `derived_from` → source/result | The record is compiled or inferred from that evidence. |
| Claim → `contradicts` → claim | Claims disagree under comparable conditions. Explain the disagreement in the record. |
| Record → `related_to` → record | There is a meaningful documented association without a stronger supported relation. |
| Mistake/issue → `corrected_by` → fix | The fix addresses that specific failure; record whether verification is pending. |
| Mistake/result → `teaches` → lesson | The evidence motivates that lesson. |
| Learning goal/issue → `resolved_by` → finding/fix | Evidence actually resolves the stated question or acceptance criteria. |
| New record → `supersedes` → old record | The new record replaces the old claim; this also retires the old record. |

Edges are directed. Do not reverse a relation just to bypass a scope check.
Broader scopes cannot point into narrower private scopes: project records can
point to relevant global knowledge, but global records cannot point into project
data. Use the CLI's scope validation; avoid copying private evidence into a broader
scope. Supersession requires the same scope and task and changes record status.

Do not invent `causes`, `supports`, `evidence_for`, or other relation names. Use
`derived_from` for provenance and describe the exact inference and uncertainty
in the record. Do not infer causal edges from proximity, chronology, shared terms,
or a common source directory.
