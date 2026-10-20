# Evidence and investigations

Use when saving experiments, audits, measurements, competing claims, or a chain
of investigation. Keep the vocabulary domain-neutral; training is one example.

## Separate roles and preserve limits

| Role | What to save |
| --- | --- |
| Fact | What was observed, measurement/units, test conditions, and exact source. |
| Fix | What changed, which failure it targets, verification performed, and remaining limits. |
| Lesson | A concrete trigger, recommended prevention, and evidence behind it. |
| Learning goal | An unresolved question and what evidence would resolve it. |

These roles do not require creating four records for every event. Save only
durable knowledge that is useful later, updating existing records where appropriate.

Use `verified` only for a claim checked against evidence, with sources and
verification details. A measured result can be verified while its proposed
explanation remains inferred. Separate those claims when needed. Reported evidence
is not weaker merely because it is inconvenient, and an authoritative-looking
report is not verified until checked.

For experimental results, preserve relevant dataset/version, model or artifact
identity, configuration, evaluation method, sample count, and applicable limits
when available. Mark missing details as unknown rather than guessing. A result on
synthetic examples does not establish general performance. A tested fix does not
establish correctness outside the tested conditions.

## Provenance is not independent confirmation

A compiled report and its raw result share an evidence lineage. Connect
`report derived_from result`; do not count the two as independent confirmations.
Repeated copies, summaries, or agent retellings do not create new evidence.

For example, position-jitter training may fix a measured position sensitivity
while an equivalent-description probe still fails. Different conditions do not
automatically contradict each other. Save the measured outcomes and their limits,
link the fix to the failure it actually addresses, and keep the broader question
open. Token IDs changing with whitespace establishes a tokenization difference;
it does not alone prove the cause of a model's failure.

An investigation's sequence can be described in its parent record. Use explicit
membership or documented association to connect steps, without implying that the
earlier finding caused the later one. Directory-layout or memory-writer fixes
belong to operational components; they do not establish model accuracy or explain
an experimental outcome simply because they enabled the run.

Do not mark a goal `resolved_by` merely because another experiment ran. Check
its stated question or acceptance criteria. Preserve failures and uncertainty;
use `supersedes` only for replacement, not to hide inconvenient observations.
