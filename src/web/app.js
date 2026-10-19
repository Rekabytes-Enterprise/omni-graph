import { filterGraph, degrees } from './graph.js';
import { GraphView } from './view.js';
import { identity } from './types.js';
const $ = id => document.getElementById(id);
let data = { graph: { memories: [], edges: [] }, active: [] }, graph = data.graph;
let selected = null, loading = false, first = true, lastSignature = '';
let flow = !matchMedia('(prefers-reduced-motion: reduce)').matches;
let view;
function text(tag, content, className) {
  const el = document.createElement(tag); el.textContent = content;
  if (className) el.className = className;
  return el;
}
function select(id) {
  selected = id; view?.select(id);
  for (const button of $('memories').children) button.classList.toggle('selected', button.dataset.id === id);
  details();
}
function details() {
  const m = graph.memories.find(m => m.id === selected);
  $('details').hidden = !m;
  const body = $('detail-body'); body.replaceChildren();
  if (!m) return;
  const type = identity(m.type);
  $('details').style.setProperty('--type-color', type.color);
  const emblem = text('div', type.icon, 'detail-emblem'); body.append(emblem, text('h2', m.title));
  const badges = text('div', '', 'badges');
  for (const value of [m.type, m.scope, m.evidence_level, m.progress || m.status]) badges.append(text('span', value.replaceAll('_', ' '), value === m.type ? 'badge type-badge' : 'badge'));
  body.append(badges);
  const section = (label, value) => {
    if (!value || (Array.isArray(value) && !value.length)) return;
    body.append(text('h3', label, 'detail-label'), text('p', Array.isArray(value) ? value.join('\n') : value, 'detail-text'));
  };
  section('Summary', m.summary); section('Memory', m.body); section('Context', m.context);
  section('Failure', m.failure); section('Cause', m.cause); section('Prevention', m.prevention);
  section('Verification', m.verification); section('Triggers', m.triggers);
  section('Topics', m.topics); section('Keywords', m.keywords); section('Sources', m.sources);
  section('Updated', new Date(m.updated_at).toLocaleString());
  section('Verified', m.verified_at && new Date(m.verified_at).toLocaleString());
  section('Review after', m.review_after && new Date(m.review_after).toLocaleString());
  section('Expires', m.expires_at && new Date(m.expires_at).toLocaleString());
  const edges = graph.edges.filter(e => e.from === m.id || e.to === m.id);
  if (edges.length) body.append(text('h3', 'Connections', 'detail-label'));
  for (const e of edges) {
    const target = graph.memories.find(n => n.id === (e.from === m.id ? e.to : e.from));
    if (!target) continue;
    const button = text('button', target.title, 'related');
    button.append(text('small', `${e.from === m.id ? '→' : '←'} ${e.relation.replaceAll('_', ' ')}`));
    button.onclick = () => select(target.id); body.append(button);
  }
  body.append(text('p', m.id, 'memory-id'));
}
function render(fit = false) {
  graph = filterGraph(data.graph, new Set(data.active), { query: $('search').value, scope: $('scope').value, kind: $('kind').value, inactive: $('inactive').checked });
  const list = $('memories'); list.replaceChildren();
  $('count').textContent = graph.memories.length;
  $('empty').hidden = graph.memories.length > 0;
  $('empty').querySelector('h2').textContent = data.graph.memories.length ? 'No matching memories' : 'Your graph starts here';
  $('empty').querySelector('p').textContent = data.graph.memories.length ? 'Try another search or change the scope and type filters.' : 'Save a memory with your agent or the CLI. Connections will appear as your knowledge grows.';
  for (const m of graph.memories) {
    const type = identity(m.type), color = type.color;
    const button = text('button', '', 'memory'); button.dataset.id = m.id; button.style.setProperty('--type-color', color);
    const glyph = text('span', type.icon, 'type-icon'); glyph.setAttribute('aria-hidden', 'true'); button.append(glyph);
    const copy = text('span', '', 'memory-copy');
    copy.append(text('span', m.title, 'memory-title'));
    const meta = text('span', '', 'memory-meta'), dot = text('span', '', 'dot'); dot.style.backgroundColor = color;
    meta.append(dot, document.createTextNode(`${type.label} · ${m.scope}`)); copy.append(meta); button.append(copy);
    button.onclick = () => select(m.id); list.append(button);
  }
  if (!graph.memories.length) list.append(text('p', data.graph.memories.length ? 'No memories match these filters.' : 'Your first memory is a conversation away.', 'list-empty'));
  if (!graph.memories.some(m => m.id === selected)) selected = null;
  $('total-nodes').textContent = graph.memories.length;
  $('total-edges').textContent = graph.edges.length;
  const types = [...new Set(data.graph.memories.map(m => m.type))].sort();
  $('total-types').textContent = new Set(graph.memories.map(m => m.type)).size;
  const legend = $('legend'); legend.replaceChildren();
  for (const kind of types) {
    const type = identity(kind), count = graph.memories.filter(m => m.type === kind).length;
    const chip = text('button', '', 'legend-chip'); chip.style.setProperty('--type-color', type.color);
    chip.setAttribute('aria-pressed', String($('kind').value === kind));
    chip.append(text('span', type.icon, 'legend-symbol'), text('span', type.label), text('span', String(count), 'legend-count'));
    chip.onclick = () => { $('kind').value = $('kind').value === kind ? '' : kind; render(true); };
    legend.append(chip);
  }
  view?.setGraph(graph, degrees(graph), fit); select(selected);
}
async function refresh() {
  if (loading) return;
  loading = true;
  try {
    const response = await fetch('/api/graph', { cache: 'no-store' });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const next = await response.json(), signature = JSON.stringify(next);
    $('project').textContent = next.project.name;
    $('connection').textContent = '● Connected · local'; $('connection').classList.remove('error');
    if (signature !== lastSignature) {
      data = next; lastSignature = signature;
      const previous = $('kind').value;
      $('kind').replaceChildren(new Option('All types', ''), ...[...new Set(data.graph.memories.map(m => m.type))].sort().map(k => new Option(identity(k).label, k)));
      $('kind').value = previous;
      render(first); first = false;
    }
  } catch (error) {
    $('connection').textContent = `Disconnected · ${error.message}`; $('connection').classList.add('error');
  } finally { loading = false; }
}
try {
  view = new GraphView($('graph'), { select, zoom: value => { $('zoom').textContent = value + '%'; } });
} catch (error) {
  $('graph').append(text('p', '3D graphics could not start. Enable WebGL2 / hardware acceleration in your browser, or use omni-graph tui. Memory search and details are still available.', 'graphics-error'));
  console.error(error);
}
for (const id of ['search', 'scope', 'kind', 'inactive']) $(id).addEventListener(id === 'search' ? 'input' : 'change', () => render(true));
$('fit').onclick = () => view?.fit(true);
$('refresh').onclick = refresh;
$('close').onclick = () => select(null);
function setFlow() { view?.setFlow(flow); $('flow').textContent = flow ? 'Pause flow' : 'Resume flow'; $('flow').setAttribute('aria-pressed', String(flow)); }
$('flow').onclick = () => { flow = !flow; setFlow(); };
for (const [id, factor] of [['zoom-in', 1.2], ['zoom-out', 1 / 1.2]]) $(id).onclick = () => view?.zoom(factor);
document.addEventListener('keydown', e => {
  if (e.target.matches('input,select,textarea') || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key.toLowerCase() === 'f') view?.fit(true);
  if (e.key === 'Escape') select(null);
});
document.addEventListener('visibilitychange', () => { if (!document.hidden) refresh(); });
setFlow(); refresh(); setInterval(() => { if (!document.hidden && !view?.drag) refresh(); }, 4000);
