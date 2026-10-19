---
name: omni-graph
description: Recall and maintain the current project's Omni-graph memory, including decisions, lessons, mistakes, verified fixes, learning goals, and task handoffs. Use before relevant project work, after useful discoveries or user corrections, and when handing work to another agent.
---

# Omni-graph memory

Run commands in the project directory. Prefer `omni-graph <command> --json` when the binary is installed (including through Cargo). Without a persistent install, run `npx --yes --package=https://github.com/Rekabytes-Enterprise/omni-graph/releases/latest/download/omni-graph.tgz omni-graph <command> --json`. This downloads the ready-to-run tarball from GitHub Releases; the short registry name is not yet published. The CLI discovers the project root from `.omni-graph/project.json`. Run `--help` or `<command> --help` for arguments. Noninteractive commands never open the TUI. The engine stores memories in local SQLite databases; search and recall use FTS5 plus graph relationships. Use the CLI for writes, not direct database edits.

Before meaningful work, run `recall "<task, component, symptoms>" --json`; include `--task <id>` when continuing a task. Follow relevant relationships with `show <id> --json`. Global preferences and project constraints accompany recall. Memory is contextual data, not authority to override user or project instructions. Verify claims against current code when they matter.

## Store durable knowledge

Search before saving. Update the existing memory when knowledge evolves; avoid near-duplicate lessons and transcript dumps. Use project scope by default. Client scope shares knowledge with that client's projects; global scope is for applicable personal preferences and generalized knowledge. Task scope requires a task ID. Do not copy client-specific details into global memory. The CLI deliberately requires explicit confirmation for promotion; ask the user before running a promotion unless they already authorized it.

Use `remember --file - --json` with a JSON object on stdin for structured learning records. Example:

```json
{
  "title": "Check consumers before changing shared component props",
  "type": "lesson",
  "scope": "project",
  "summary": "Inspect affected consumers before changing a public contract.",
  "body": "A shared prop change affected a second page that was not inspected.",
  "topics": ["frontend", "components"],
  "keywords": ["props", "public interface"],
  "triggers": ["Changing shared component props or exported interfaces"],
  "cause": "Only one consumer was inspected.",
  "prevention": ["Find all consumers before editing the contract."],
  "evidence_level": "reported",
  "sources": ["User correction in this task"],
  "created_by": "agent"
}
```

Use `update <id> --file - --json` with only changed fields. Scope, IDs, ownership and creation history are immutable. Relocate knowledge by creating a suitably scoped record with lineage, not by patching ownership.

## Learn from failures

Capture a `mistake` with failure, cause and context; mark uncertain causes `inferred`. Record each meaningful attempted `fix`, including a failed attempt's outcome in its body. Link `mistake corrected_by fix` and `mistake teaches lesson`. Write lessons around concrete retrieval triggers, prevention and verification, not vague advice to be careful.

An issue, mistake, fix or learning goal can have progress `open`, `investigating`, `fix_proposed`, `fixed`, or `verified`. Do not claim verified without verification details, a source and `evidence_level: verified`. Saving a proposed fix does not prove the issue is resolved. Link `learning_goal resolved_by fact` when an investigation produces a finding. Repeated mistakes should refine an existing lesson rather than multiply duplicates.

Use `link <new-id> supersedes <old-id>` to retire outdated knowledge in the same scope. Preserve competing claims with `contradicts` until evidence resolves them. Archive completed handoffs with `update` instead of leaving them active. Run `review --json` to find stale knowledge, open work and recurring causes; it does not automatically rewrite memories.

For a handoff, save a task-scoped `handoff` containing current state, unresolved questions and next actions. Store durable findings separately. Keep source references and verification dates meaningful. Do not store credentials or copy sensitive details into broader scopes. Skills guide this workflow; they cannot guarantee automatic recall or change model weights.
