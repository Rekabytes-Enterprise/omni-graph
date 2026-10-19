// Pure graph/camera operations shared by the browser and interaction tests.
export function filterGraph(graph, active, { query = '', scope = '', kind = '', inactive = false } = {}) {
  const terms = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
  const memories = graph.memories.filter(m => (inactive || active.has(m.id)) && (!scope || m.scope === scope) && (!kind || m.type === kind) && terms.every(t => [m.title, m.summary, m.body, ...(m.topics || []), ...(m.keywords || [])].join(' ').toLocaleLowerCase().includes(t)));
  const ids = new Set(memories.map(m => m.id));
  return { memories, edges: graph.edges.filter(e => ids.has(e.from) && ids.has(e.to)) };
}
export function degrees(graph) {
  const neighbors = new Map(graph.memories.map(m => [m.id, new Set()]));
  for (const e of graph.edges) if (e.from !== e.to) {
    neighbors.get(e.from)?.add(e.to); neighbors.get(e.to)?.add(e.from);
  }
  return new Map([...neighbors].map(([id, set]) => [id, set.size]));
}
export function radius(degree) { return 7 + Math.min(11, Math.sqrt(degree || 0) * 2.1); }
export function seedPositions(memories, positions) {
  const sorted = [...memories].sort((a, b) => a.id.localeCompare(b.id));
  sorted.forEach((m, i) => {
    if (!positions.has(m.id)) {
      const angle = i * 2.399963229728653;
      const r = 65 * Math.cbrt(i + 1);
      const polar = Math.acos(1 - 2 * (i + .5) / Math.max(1, sorted.length));
      positions.set(m.id, { x: Math.cos(angle) * Math.sin(polar) * r, y: Math.cos(polar) * r, z: Math.sin(angle) * Math.sin(polar) * r, vx: 0, vy: 0, vz: 0 });
    }
  });
}
export function relax(graph, positions, degree, held) {
  const ids = graph.memories.map(m => m.id);
  // Bound repulsion work for large graphs; graph edges remain exact.
  const stride = Math.max(1, Math.ceil(ids.length / 350));
  for (let i = 0; i < ids.length; i++) {
    const a = positions.get(ids[i]);
    for (let j = i + 1; j < ids.length; j += stride) {
      const b = positions.get(ids[j]);
      let dx = a.x - b.x, dy = a.y - b.y, dz = a.z - b.z;
      if (dx === 0 && dy === 0 && dz === 0) dx = 1;
      const d2 = Math.max(64, dx * dx + dy * dy + dz * dz), d = Math.sqrt(d2);
      const f = Math.min(3, 1000 / d2);
      a.vx += dx / d * f; a.vy += dy / d * f; a.vz += dz / d * f;
      b.vx -= dx / d * f; b.vy -= dy / d * f; b.vz -= dz / d * f;
    }
  }
  for (const e of graph.edges) {
    if (e.from === e.to) continue;
    const a = positions.get(e.from), b = positions.get(e.to);
    const dx = b.x - a.x, dy = b.y - a.y, dz = b.z - a.z, d = Math.max(1, Math.hypot(dx, dy, dz));
    const rest = 95 + radius(degree.get(e.from)) + radius(degree.get(e.to));
    const f = (d - rest) * .012;
    a.vx += dx / d * f; a.vy += dy / d * f; a.vz += dz / d * f; b.vx -= dx / d * f; b.vy -= dy / d * f; b.vz -= dz / d * f;
  }
  for (const id of ids) {
    const p = positions.get(id);
    if (id === held) { p.vx = 0; p.vy = 0; p.vz = 0; continue; }
    p.vx = (p.vx - p.x * .0006) * .78; p.vy = (p.vy - p.y * .0006) * .78;
    p.vz = (p.vz - p.z * .0006) * .78;
    p.x += Math.max(-5, Math.min(5, p.vx)); p.y += Math.max(-5, Math.min(5, p.vy)); p.z += Math.max(-5, Math.min(5, p.vz));
  }
}
export function bounds(memories, positions) {
  if (!memories.length) return { center: { x: 0, y: 0, z: 0 }, radius: 80 };
  const points = memories.map(m => positions.get(m.id));
  const axis = key => (Math.min(...points.map(p => p[key])) + Math.max(...points.map(p => p[key]))) / 2;
  const center = { x: axis('x'), y: axis('y'), z: axis('z') };
  return { center, radius: Math.max(40, ...points.map(p => Math.hypot(p.x-center.x,p.y-center.y,p.z-center.z))) + 35 };
}
