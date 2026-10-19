import test from 'node:test';
import assert from 'node:assert/strict';
import { filterGraph, degrees, radius, seedPositions, relax, bounds } from '../src/web/graph.js';
const graph = { memories: [
  { id: 'a', title: 'API decision', body: 'Use Rust', type: 'decision', scope: 'project' },
  { id: 'b', title: 'Previous failure', keywords: ['retry'], type: 'mistake', scope: 'project' },
  { id: 'c', title: 'Global preference', type: 'preference', scope: 'global' },
], edges: [{ from: 'a', to: 'b' }, { from: 'b', to: 'a' }, { from: 'a', to: 'a' }, { from: 'a', to: 'c' }] };
test('browser filters trim edges and respect active IDs, scopes and all search terms', () => {
  assert.deepEqual(filterGraph(graph, new Set(['a', 'b']), { query: 'api rust' }).memories.map(m => m.id), ['a']);
  assert.equal(filterGraph(graph, new Set(['a', 'b']), { scope: 'global' }).memories.length, 0);
  assert.deepEqual(filterGraph(graph, new Set(['a']), { inactive: true, kind: 'mistake', query: 'retry' }).memories.map(m => m.id), ['b']);
  const filtered = filterGraph(graph, new Set(['a', 'b']), { scope: 'project' });
  assert.equal(filtered.edges.length, 3);
});
test('solid node sizes count distinct neighbors once and exclude self links', () => {
  const degree = degrees(graph);
  assert.equal(degree.get('a'), 2); assert.equal(degree.get('b'), 1);
  assert.ok(radius(2) > radius(1)); assert.ok(radius(1e9) <= 18);
});
test('3D bounds include every point and a nonzero empty-graph radius', () => {
  const positions = new Map(); seedPositions(graph.memories, positions);
  assert.ok([...positions.values()].some(p => p.z !== 0));
  const box = bounds(graph.memories, positions);
  for (const p of positions.values()) assert.ok(Math.hypot(p.x-box.center.x,p.y-box.center.y,p.z-box.center.z) < box.radius);
  assert.ok(bounds([], new Map()).radius > 0);
});
test('layout remains finite, preserves held nodes and does not write graph records', () => {
  const original = structuredClone(graph), positions = new Map(), degree = degrees(graph);
  seedPositions(graph.memories, positions);
  const held = { ...positions.get('a') };
  for (let i = 0; i < 200; i++) relax(graph, positions, degree, 'a');
  assert.equal(positions.get('a').x, held.x); assert.equal(positions.get('a').y, held.y); assert.equal(positions.get('a').z, held.z);
  for (const p of positions.values()) assert.ok(Object.values(p).every(Number.isFinite));
  assert.deepEqual(graph, original);
  const before = structuredClone(positions.get('b'));
  seedPositions([...graph.memories, { id: 'new' }], positions);
  assert.deepEqual(positions.get('b'), before);
});

test('memory type colors distinguish issues, mistakes, fixes and lessons', async () => {
  const { TYPES, identity } = await import('../src/web/types.js');
  assert.equal(Object.keys(TYPES).length, 13);
  assert.equal(new Set(['issue', 'mistake', 'fix', 'lesson', 'decision'].map(k => identity(k).color)).size, 5);
  for (const kind of Object.keys(TYPES)) {
    assert.match(identity(kind).color, /^#[0-9a-f]{6}$/);
    assert.ok(identity(kind).label && identity(kind).icon);
  }
  assert.ok(identity('future_type').color);
});
