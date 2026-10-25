#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fixture = JSON.parse(readFileSync(join(repository, 'benchmarks/recall-fixture.json'), 'utf8'));

export function metrics(returned, relevant, k) {
  if (!Number.isInteger(k) || k < 1) throw new Error('k must be a positive integer');
  const expected = new Set(relevant);
  if (!expected.size) throw new Error('A query needs at least one relevant record');
  const top = returned.slice(0, k);
  const seen = new Set();
  let hits = 0;
  let first = 0;
  for (let i = 0; i < top.length; i += 1) {
    if (!seen.has(top[i]) && expected.has(top[i])) {
      hits += 1;
      if (!first) first = i + 1;
    }
    seen.add(top[i]);
  }
  return { precision_at_k: hits / k, recall_at_k: hits / expected.size, reciprocal_rank: first ? 1 / first : 0 };
}

export function percentile(values, fraction) {
  if (!values.length || fraction <= 0 || fraction > 1) throw new Error('Invalid percentile input');
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(fraction * sorted.length) - 1];
}

function average(values) { return values.reduce((sum, value) => sum + value, 0) / values.length; }
function diskBytes(path) {
  const stat = statSync(path);
  return stat.isDirectory() ? readdirSync(path).reduce((total, child) => total + diskBytes(join(path, child)), 0) : stat.size;
}
function command(binary, context, args, input) {
  const start = performance.now();
  const result = spawnSync(binary, ['--directory', context.project, ...args], {
    env: { ...process.env, HOME: context.home, USERPROFILE: context.home, OMNI_GRAPH_HOME: context.store },
    cwd: context.project, input: input === undefined ? undefined : JSON.stringify(input), encoding: 'utf8',
    timeout: 120_000, maxBuffer: 32 * 1024 * 1024,
  });
  const ms = performance.now() - start;
  if (result.error || result.status !== 0) {
    throw new Error(`${binary} ${args.join(' ')} failed: ${result.error?.message ?? (result.stderr || result.stdout)}`);
  }
  return { value: JSON.parse(result.stdout), ms, bytes: Buffer.byteLength(result.stdout) };
}
function context(root) {
  const selected = { project: join(root, 'project'), home: join(root, 'home'), store: join(root, 'home', '.omni-graph') };
  mkdirSync(selected.project, { recursive: true });
  mkdirSync(selected.home, { recursive: true });
  return selected;
}
function seed(binary, root, size, imageBytes) {
  const ctx = context(root);
  command(binary, ctx, ['init', '--name', 'Synthetic recall benchmark', '--agent', 'none', '--json']);
  const keys = {};
  function save(key, memory) {
    const result = command(binary, ctx, ['remember', '--file', '-', '--json'], {
      scope: 'project', evidence_level: 'reported', sources: ['Synthetic benchmark fixture, not production evidence'],
      created_by: 'benchmark', ...memory,
    }).value;
    keys[key] = result.id;
    if (!result.id) throw new Error('remember response has no memory ID');
  }
  for (const record of fixture.memories) {
    const { key, links, ...memory } = record;
    if (links) memory.links = links.map(link => ({ ...link, from: keys[link.from] }));
    save(key, memory);
  }
  for (let i = fixture.memories.length; i < size; i += 1) {
    const [title, body] = fixture.distractors[(i - fixture.memories.length) % fixture.distractors.length];
    save(`noise_${i}`, { type: 'fact', title: `${title} ${i}`, body: `${body} Synthetic record ${i}.` });
    if (i % 100 === 0) process.stderr.write(`Seeded ${i}/${size} records\n`);
  }
  if (imageBytes) {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j6WQAAAAASUVORK5CYII=', 'base64');
    const file = join(root, 'synthetic-reference.png');
    writeFileSync(file, Buffer.concat([png, Buffer.alloc(Math.max(0, imageBytes - png.length))]));
    command(binary, ctx, ['image', 'add', keys.theme, '--file', file, '--caption', 'Synthetic padded PNG for storage-cost measurement', '--json']);
    rmSync(file);
  }
  const other = { ...ctx, project: join(root, 'other-project') };
  mkdirSync(other.project);
  command(binary, other, ['init', '--name', 'Out-of-scope fixture', '--agent', 'none', '--json']);
  const hidden = command(binary, other, ['remember', '--file', '-', '--json'], {
    scope: 'project', type: 'fact', title: 'Webhook retries duplicate charges SQLite WAL backup checkout keyboard focus',
    body: 'Deliberately matching terms from an unrelated project; must never appear in the selected project.',
  }).value.id;
  return { keys, hidden };
}

function identity(binary) {
  const result = spawnSync(binary, ['--version'], { encoding: 'utf8', timeout: 10_000 });
  if (result.status !== 0) throw new Error(`Cannot execute ${binary}`);
  return { path: binary, version: result.stdout.trim(), sha256: createHash('sha256').update(readFileSync(binary)).digest('hex') };
}

export function runBenchmark(options) {
  const root = mkdtempSync(join(tmpdir(), 'omni-recall-benchmark-'));
  const binaries = options.baseline ? [['baseline', options.baseline], ['candidate', options.binary]] : [['candidate', options.binary]];
  const report = {
    benchmark_version: 1, fixture_version: fixture.version, generated_at: new Date().toISOString(),
    platform: `${process.platform}/${process.arch}`, node: process.version,
    binaries: Object.fromEntries(binaries.map(([label, binary]) => [label, identity(binary)])),
    options: { sizes: options.sizes, repeats: options.repeats, k: options.k, image_bytes: options.imageBytes },
    methodology: 'CLI wall-clock latency including process startup; one warmup per query/depth; nearest-rank percentiles; macro-average quality; snapshots cloned from one seeded fixture per size.',
    limitations: ['Synthetic labels are authored expectations, not a human relevance study.', 'Paraphrase and overlapping distractors can fail lexical recall; these failures remain in the report.', 'Warm-cache timings are machine/load/build dependent and exclude initial schema migration and fixture creation.', 'Pinned global preference is judged relevant to every query; graph_evidence intentionally judges the matching issue as navigation context rather than the requested answer.'],
    runs: [],
  };
  try {
    for (const size of options.sizes) {
      process.stderr.write(`Preparing ${size}-record corpus\n`);
      const template = join(root, `template-${size}`);
      const { keys, hidden } = seed(binaries[0][1], template, size, options.imageBytes);
      const reverse = Object.fromEntries(Object.entries(keys).map(([key, id]) => [id, key]));
      const states = binaries.map(([label, binary]) => {
        const path = join(root, `${size}-${label}`);
        cpSync(template, path, { recursive: true });
        const ctx = context(path);
        const run = { binary: label, corpus_size: size, other_project_records: 1, fixture_bytes: diskBytes(path), samples: [], queries: [] };
        report.runs.push(run);
        return { binary, ctx, run };
      });
      for (const query of fixture.queries) {
        for (const depth of [0, 1]) {
          const args = ['recall', query.text, '--depth', String(depth), '--limit', String(options.k), '--json'];
          if (query.task) args.push('--task', query.task);
          for (const state of states) command(state.binary, state.ctx, args);
          const samples = new Map(states.map(state => [state, []]));
          for (let round = 0; round < options.repeats; round += 1) {
            for (const state of round % 2 ? [...states].reverse() : states) {
              const result = command(state.binary, state.ctx, args);
              if (!Array.isArray(result.value.results)) throw new Error('Recall response lacks results');
              const ids = result.value.results.map(hit => hit.memory.id);
              if (ids.includes(hidden) || ids.includes(keys.task_other) || (!query.task && ids.includes(keys.task_current))) {
                throw new Error(`Scope isolation failure: ${query.key}`);
              }
              if (new Set(ids).size !== ids.length) throw new Error(`Duplicate recall records: ${query.key}`);
              const returned = ids.map(id => reverse[id] ?? `unknown:${id}`);
              samples.get(state).push({ ms: result.ms, bytes: result.bytes, ...metrics(returned, query.relevant, options.k), returned });
            }
          }
          for (const state of states) {
            const values = samples.get(state);
            state.run.samples.push(...values.map(value => ({ query: query.key, depth, ...value })));
            state.run.queries.push({ key: query.key, text: query.text, depth, relevant: query.relevant,
              precision_at_k: average(values.map(v => v.precision_at_k)), recall_at_k: average(values.map(v => v.recall_at_k)),
              mrr: average(values.map(v => v.reciprocal_rank)), latency_ms_p50: percentile(values.map(v => v.ms), .5),
              latency_ms_p95: percentile(values.map(v => v.ms), .95), response_bytes_mean: average(values.map(v => v.bytes)),
              returned: values[0].returned, stable_repetitions: values.every(v => JSON.stringify(v.returned) === JSON.stringify(values[0].returned)),
            });
          }
        }
      }
      for (const { run } of states) {
        run.summary = [0, 1].map(depth => {
          const queries = run.queries.filter(query => query.depth === depth);
          const samples = run.samples.filter(sample => sample.depth === depth);
          return { depth, precision_at_k: average(queries.map(q => q.precision_at_k)), recall_at_k: average(queries.map(q => q.recall_at_k)),
            mrr: average(queries.map(q => q.mrr)), latency_ms_p50: percentile(samples.map(s => s.ms), .5),
            latency_ms_p95: percentile(samples.map(s => s.ms), .95), response_bytes_mean: average(samples.map(s => s.bytes)) };
        });
      }
    }
    return report;
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function main() {
  const { values } = parseArgs({ options: {
    binary: { type: 'string' }, baseline: { type: 'string' }, sizes: { type: 'string', default: '100' },
    repeats: { type: 'string', default: '3' }, k: { type: 'string', default: '5' },
    'image-bytes': { type: 'string', default: '0' }, output: { type: 'string' }, help: { type: 'boolean' },
  } });
  if (values.help) {
    console.log('node scripts/benchmark-recall.mjs --binary /absolute/omni-graph [--baseline /absolute/old-binary] [--sizes 100,1000] [--repeats 3] [--k 5] [--image-bytes 4194304] [--output report.json]');
    return;
  }
  if (!values.binary || !isAbsolute(values.binary) || (values.baseline && !isAbsolute(values.baseline))) throw new Error('Pass absolute --binary and --baseline paths');
  const options = { binary: values.binary, baseline: values.baseline, sizes: values.sizes.split(',').map(Number), repeats: Number(values.repeats), k: Number(values.k), imageBytes: Number(values['image-bytes']) };
  if (options.sizes.some(size => !Number.isInteger(size) || size < fixture.memories.length) || !Number.isInteger(options.repeats) || options.repeats < 1 || !Number.isInteger(options.k) || options.k < 1 || options.k > 100 || !Number.isInteger(options.imageBytes) || options.imageBytes < 0 || options.imageBytes > 8 * 1024 * 1024) throw new Error('Invalid sizes/repeats/k/image-bytes');
  const output = `${JSON.stringify(runBenchmark(options), null, 2)}\n`;
  if (values.output) writeFileSync(resolve(values.output), output);
  else process.stdout.write(output);
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
