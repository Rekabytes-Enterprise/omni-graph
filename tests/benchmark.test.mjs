import test from 'node:test';
import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { metrics, percentile, runBenchmark } from '../scripts/benchmark-recall.mjs';

test('quality metrics count ranked unique hits and penalize missing results', () => {
  assert.deepEqual(metrics(['wrong', 'a', 'a', 'b', 'outside'], ['a', 'b', 'c'], 4), {
    precision_at_k: .5, recall_at_k: 2 / 3, reciprocal_rank: .5,
  });
  assert.deepEqual(metrics([], ['a'], 5), { precision_at_k: 0, recall_at_k: 0, reciprocal_rank: 0 });
  assert.equal(metrics(['a'], ['a'], 5).precision_at_k, .2);
  assert.throws(() => metrics([], [], 5));
  assert.throws(() => metrics([], ['a'], 0));
});

test('nearest-rank percentiles preserve the original samples', () => {
  const values = [90, 10, 30, 20];
  assert.equal(percentile(values, .5), 20);
  assert.equal(percentile(values, .95), 90);
  assert.deepEqual(values, [90, 10, 30, 20]);
  assert.throws(() => percentile([], .5));
});


test('real recall keeps scoped evidence and retrieves linked fixes in the small fixture', { timeout: 120_000 }, () => {
  const binary = process.env.OMNI_GRAPH_BENCHMARK_BINARY || resolve('target/debug', process.platform === 'win32' ? 'omni-graph.exe' : 'omni-graph');
  const report = runBenchmark({ binary, sizes: [20], repeats: 1, k: 5, imageBytes: 0 });
  const run = report.runs[0];
  assert.equal(run.queries.length, 16);
  const evidence = run.queries.find(query => query.key === 'graph_evidence' && query.depth === 1);
  assert.ok(evidence.returned.includes('payment_fix'), JSON.stringify(evidence));
  const task = run.queries.find(query => query.key === 'task' && query.depth === 1);
  assert.ok(task.returned.includes('task_current'));
  assert.ok(run.summary.find(summary => summary.depth === 1).recall_at_k >= .5);
  assert.ok(run.samples.every(sample => sample.bytes > 0 && sample.ms > 0));
});
