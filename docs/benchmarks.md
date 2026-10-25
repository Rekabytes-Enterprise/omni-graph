# Recall benchmark

Run the benchmark against an explicit local executable. It creates synthetic memories
through the real CLI, measures actual JSON recall responses, and removes its temporary
stores afterward. It does not read or migrate your memory stores. Both `HOME` and
`OMNI_GRAPH_HOME` point at its disposable workspace for every memory command.

```sh
cargo build --release
node scripts/benchmark-recall.mjs \
  --binary "$PWD/target/release/omni-graph" \
  --sizes 100,1000 --repeats 5 --k 5 \
  --output /tmp/recall-candidate.json
```

To compare binaries, preserve the baseline executable before rebuilding and pass
both absolute paths. Use the same build profile for performance comparisons.

```sh
node scripts/benchmark-recall.mjs \
  --baseline /absolute/path/to/baseline/omni-graph \
  --binary /absolute/path/to/candidate/omni-graph \
  --sizes 100,1000 --repeats 5 \
  --output /tmp/recall-comparison.json
```

The runner seeds one fixture with the baseline (or candidate when used alone), then
clones it for each executable. IDs, timestamps, relationships, and distractor order
are identical between compared binaries. Any migration during the first warmup is
outside measured latency. Compare the recorded executable SHA256 values as well as
version strings: development builds can share a package version.

## Workload and judgments

The checked-in [fixture](../benchmarks/recall-fixture.json) defines ten core memories,
eight queries, and repeating distractor templates. The requested corpus size counts
the global preference plus the selected project's records, including task records.
One additional record in an unrelated project deliberately matches query terms.
The runner fails if that record, the unrelated task, or an unselected task leaks.

Queries cover exact lexical matches, a paraphrase, a linked fix/lesson, timezone
boundaries, migration instructions, visual decisions, cache behavior, and a selected
task. Distractors intentionally share vocabulary with relevant records. Each query
runs with graph depth zero and one, separating lexical retrieval from graph expansion.
Stable fixture keys map generated memory IDs to authored relevance judgments.

The global delivery preference is judged relevant to every query. For
`graph_evidence`, the matching issue is treated as navigation context; the desired
answer consists of its fix, lesson, and applicable preference. This is a narrower
judgment than the `exact` query despite identical query text. Inspect per-query
results before drawing conclusions from the aggregate.

These are synthetic judgments, not measurements of real user satisfaction or broad
semantic understanding. The paraphrase can expose lexical limitations, and shared
words can rank distractors highly. The report preserves those failures. Adding
embeddings or a reranker would require new evaluated implementations; this benchmark
does not assume either is present.

## Metrics

- **Precision@k:** unique relevant IDs among the first k results divided by k.
  Returning fewer than k results does not reduce the denominator.
- **Recall@k:** unique relevant IDs among the first k results divided by all judged
  relevant IDs for that query.
- **MRR:** mean reciprocal rank of the first relevant result, or zero when absent.
- **Latency p50/p95:** nearest-rank percentiles of process wall-clock time, including
  CLI startup, store access, retrieval, serialization, and process exit.
- **Response bytes:** UTF-8 stdout size, including the response envelope and any image
  metadata. Image bytes are not expected in recall output.

Quality aggregates are macro-averages across queries. Latency aggregates pool all
measured samples for a corpus size and graph depth. Each query/depth receives one
unmeasured warmup per binary. Measurement alternates binary order on successive
rounds to reduce ordering bias. Raw samples, returned fixture keys, and repetition
stability remain in the JSON report. No universal latency threshold is enforced;
hardware, cache state, build profile, background load, and process startup matter.

## Image-heavy storage

```sh
node scripts/benchmark-recall.mjs \
  --baseline /absolute/path/to/baseline/omni-graph \
  --binary /absolute/path/to/candidate/omni-graph \
  --sizes 100 --image-bytes 4194304 --repeats 5 \
  --output /tmp/recall-image-comparison.json
```

This adds one PNG attachment to the visual decision. The file is a synthetic
one-pixel PNG padded to the requested byte count, up to the image limit of 8 MiB.
It measures the cost of a larger stored BLOB, not realistic image decoding or visual
reasoning. Use zero bytes, the default, to omit the attachment. Fixture creation time
is excluded, and the report records total fixture file bytes before measurement.

## Validation and interpretation

```sh
node --test tests/benchmark.test.mjs
```

Metric tests cover empty results, repeated IDs, truncated result lists, rank, and
percentile calculation. A small real CLI fixture checks linked-fix and selected-task
retrieval with a recall floor of 0.5; this regression floor is not a production
quality target. The CLI runner checks response structure, duplicate results,
and scope isolation. Review quality differences and individual missed judgments;
a faster run is not an improvement if it loses useful results or scope isolation.

No production performance claim follows from a single synthetic run. Keep the full
report with the compared binary hashes and commands when reporting an improvement.
