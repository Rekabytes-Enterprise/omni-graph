import * as THREE from './vendor/three.module.js';
import { OrbitControls } from './vendor/OrbitControls.js';
import { identity } from './types.js';
import { seedPositions, relax, radius, bounds } from './graph.js';

// Owns the complete 3D scene, camera, picking and temporary node positions.
export class GraphView {
  constructor(container, { select, zoom }) {
    this.container = container; this.onSelect = select; this.onZoom = zoom;
    this.positions = new Map(); this.nodes = new Map(); this.edges = [];
    this.graph = { memories: [], edges: [] }; this.degree = new Map();
    this.selected = null; this.flow = true; this.frame = 0; this.steps = 0;
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(45, 1, 1, 30000);
    this.camera.position.set(320, 230, 500);
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.domElement.setAttribute('aria-label', '3D graph canvas');
    container.append(this.renderer.domElement);
    this.labels = document.createElement('div'); this.labels.className = 'node-labels'; container.append(this.labels);
    this.scene.add(new THREE.AmbientLight(0xe7f4ff, 1.5));
    const light = new THREE.DirectionalLight(0xffffff, 3); light.position.set(200, 400, 500); this.scene.add(light);
    const rim = new THREE.DirectionalLight(0x86b8e0, 1.5); rim.position.set(-400, -100, -200); this.scene.add(rim);
    const grid = new THREE.GridHelper(1600, 50, 0x768aab, 0x647089);
    grid.position.y = -190; grid.material.transparent = true; grid.material.opacity = .085; this.scene.add(grid);
    const glowCanvas = document.createElement('canvas'); glowCanvas.width = glowCanvas.height = 64;
    const ctx = glowCanvas.getContext('2d'), gradient = ctx.createRadialGradient(32,32,0,32,32,32);
    gradient.addColorStop(0,'rgba(255,255,255,.32)'); gradient.addColorStop(.25,'rgba(255,255,255,.12)'); gradient.addColorStop(1,'rgba(255,255,255,0)');
    ctx.fillStyle = gradient; ctx.fillRect(0,0,64,64); this.glowTexture = new THREE.CanvasTexture(glowCanvas);
    this.sphere = new THREE.SphereGeometry(1, 24, 16);
    this.ringGeometry = new THREE.RingGeometry(1.28, 1.34, 48);
    this.pulseMaterial = new THREE.MeshBasicMaterial({ color: 0xbffff0 });
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = true; this.controls.dampingFactor = .09;
    this.controls.screenSpacePanning = true; this.controls.zoomToCursor = true;
    this.controls.minDistance = 35; this.controls.maxDistance = 12000;
    this.controls.addEventListener('change', () => this.schedule());
    this.ray = new THREE.Raycaster(); this.pointer = new THREE.Vector2();
    this.plane = new THREE.Plane(); this.drag = null;
    this.bindPointer();
    this.resize = new ResizeObserver(() => {
      const width = Math.max(1, container.clientWidth), height = Math.max(1, container.clientHeight);
      this.renderer.setSize(width, height); this.camera.aspect = width / height; this.camera.updateProjectionMatrix(); this.schedule();
    });
    this.resize.observe(container);
    document.addEventListener('visibilitychange', () => { if (!document.hidden) this.schedule(); });
    this.renderer.domElement.addEventListener('webglcontextlost', e => { e.preventDefault(); container.classList.add('context-lost'); });
    this.renderer.domElement.addEventListener('webglcontextrestored', () => { container.classList.remove('context-lost'); this.schedule(); });
  }
  schedule() { if (!this.frame && !document.hidden) this.frame = requestAnimationFrame(t => this.tick(t)); }
  clear() {
    for (const { mesh, label } of this.nodes.values()) { this.scene.remove(mesh); mesh.material.dispose(); mesh.children.forEach(child => child.material?.dispose()); label.remove(); }
    for (const edge of this.edges) { this.scene.remove(edge.line, edge.pulse); edge.line.geometry.dispose(); edge.line.material.dispose(); edge.pulse.material.dispose(); }
    this.nodes.clear(); this.edges = [];
  }
  setGraph(graph, degree, fit = false) {
    if (this.drag) this.releaseDrag();
    this.clear(); this.graph = graph; this.degree = degree;
    const existing = this.positions.size; seedPositions(graph.memories, this.positions);
    const live = new Set(graph.memories.map(m => m.id));
    // Retain hidden positions for filter changes, but never alter stored memories.
    for (const m of graph.memories) {
      const color = identity(m.type).color;
      const material = new THREE.MeshStandardMaterial({ color, roughness: .32, metalness: .15, emissive: color, emissiveIntensity: .16 });
      const mesh = new THREE.Mesh(this.sphere, material); mesh.scale.setScalar(radius(degree.get(m.id))); mesh.userData.id = m.id;
      const ring = new THREE.Mesh(this.ringGeometry, new THREE.MeshBasicMaterial({ color, side: THREE.DoubleSide, transparent: true, opacity: .8 }));
      ring.visible = m.id === this.selected; mesh.add(ring);
      const halo = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.glowTexture, color, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
      halo.scale.set(6,6,1); mesh.add(halo);
      const label = document.createElement('span'); label.className = 'node-label'; label.textContent = m.title.length > 36 ? m.title.slice(0, 34) + '…' : m.title;
      label.title = m.title; label.style.setProperty('--type-color', color); this.labels.append(label); this.scene.add(mesh); this.nodes.set(m.id, { mesh, ring, label, color });
    }
    for (const e of graph.edges) {
      if (!live.has(e.from) || !live.has(e.to)) continue;
      const line = new THREE.Line(new THREE.BufferGeometry(), new THREE.LineBasicMaterial({ color: 0x667987, transparent: true, opacity: .38 }));
      const pulse = new THREE.Mesh(this.sphere, this.pulseMaterial.clone()); pulse.scale.setScalar(2.2); pulse.visible = false;
      this.scene.add(line, pulse); this.edges.push({ ...e, line, pulse, curve: null });
    }
    this.steps = 150; this.fitPending = fit || !existing;
    this.updateGeometry(); this.select(this.selected); this.schedule();
  }
  select(id) {
    this.selected = id;
    for (const [key, node] of this.nodes) {
      node.ring.visible = key === id; node.label.classList.toggle('selected', key === id);
      node.mesh.material.emissiveIntensity = key === id ? .4 : .16;
    }
    for (const edge of this.edges) {
      const selected = edge.from === id || edge.to === id;
      edge.line.material.color.set(selected ? (this.nodes.get(id)?.color || '#9cdbca') : 0x71869d); edge.line.material.opacity = selected ? .8 : .3;
      edge.pulse.material.color.set(this.nodes.get(id)?.color || '#bffff0');
      edge.pulse.visible = selected && this.flow && edge.from !== edge.to;
    }
    this.schedule();
  }
  setFlow(flow) { this.flow = flow; this.select(this.selected); }
  fit(reset = false) {
    const { center, radius: size } = bounds(this.graph.memories, this.positions);
    const target = new THREE.Vector3(center.x, center.y, center.z);
    const direction = reset ? new THREE.Vector3(.65, .38, 1).normalize() : this.camera.position.clone().sub(this.controls.target).normalize();
    const halfFov = Math.min(this.camera.fov * Math.PI / 360, Math.atan(Math.tan(this.camera.fov * Math.PI / 360) * this.camera.aspect));
    const distance = Math.max(120, size / Math.sin(halfFov) * 1.15);
    this.controls.target.copy(target); this.camera.position.copy(target).addScaledVector(direction, distance);
    this.baseDistance = distance; this.camera.lookAt(target); this.controls.update(); this.schedule();
  }
  zoom(factor) {
    this.fitPending = false;
    const offset = this.camera.position.clone().sub(this.controls.target);
    offset.setLength(THREE.MathUtils.clamp(offset.length() / factor, this.controls.minDistance, this.controls.maxDistance));
    this.camera.position.copy(this.controls.target).add(offset); this.controls.update(); this.schedule();
  }
  updateGeometry() {
    const vector = id => { const p = this.positions.get(id); return new THREE.Vector3(p.x, p.y, p.z); };
    for (const [id, node] of this.nodes) node.mesh.position.copy(vector(id));
    for (const edge of this.edges) {
      const a = vector(edge.from), b = vector(edge.to), delta = b.clone().sub(a);
      let curve;
      if (edge.from === edge.to) {
        const r = radius(this.degree.get(edge.from));
        curve = new THREE.CubicBezierCurve3(a.clone().add(new THREE.Vector3(-r, 0, 0)), a.clone().add(new THREE.Vector3(-50, 65, 30)), a.clone().add(new THREE.Vector3(50, 65, 30)), a.clone().add(new THREE.Vector3(r, 0, 0)));
      } else {
        const normal = delta.clone().cross(new THREE.Vector3(.3, 1, .2)).normalize().multiplyScalar(Math.min(32, delta.length() * .16));
        const control = a.clone().add(b).multiplyScalar(.5).add(normal);
        const start = a.clone().add(control.clone().sub(a).normalize().multiplyScalar(radius(this.degree.get(edge.from))));
        const end = b.clone().add(control.clone().sub(b).normalize().multiplyScalar(radius(this.degree.get(edge.to))));
        curve = new THREE.QuadraticBezierCurve3(start, control, end);
      }
      edge.curve = curve;
      const points = curve.getPoints(36);
      if (edge.line.geometry.attributes.position) {
        const attribute = edge.line.geometry.attributes.position;
        points.forEach((p, i) => attribute.setXYZ(i, p.x, p.y, p.z)); attribute.needsUpdate = true; edge.line.geometry.computeBoundingSphere();
      } else edge.line.geometry.setFromPoints(points);
    }
  }
  tick(time) {
    this.frame = 0;
    if (document.hidden) return;
    if (this.steps > 0) { relax(this.graph, this.positions, this.degree, this.drag?.id); this.steps--; this.updateGeometry(); }
    if (this.fitPending) { this.fit(); if (!this.steps) this.fitPending = false; }
    const changed = this.controls.update();
    let animating = false;
    for (let i = 0; i < this.edges.length; i++) {
      const edge = this.edges[i];
      if (edge.pulse.visible && edge.curve) {
        edge.pulse.position.copy(edge.curve.getPoint(((time % 2400) / 2400 + i * .173) % 1)); animating = true;
      }
    }
    const labels = [];
    for (const [id, node] of this.nodes) {
      node.ring.quaternion.copy(this.camera.quaternion);
      const p = node.mesh.position.clone().project(this.camera);
      const visible = p.z > -1 && p.z < 1 && Math.abs(p.x) < 1.1 && Math.abs(p.y) < 1.1;
      node.label.hidden = !visible;
      if (visible) {
        const distance = this.camera.position.distanceTo(node.mesh.position);
        const pixelRadius = node.mesh.scale.x * this.container.clientHeight / (2 * Math.tan(this.camera.fov * Math.PI / 360) * distance);
        const x = (p.x + 1) * this.container.clientWidth / 2, y = (-p.y + 1) * this.container.clientHeight / 2 + pixelRadius + 8;
        const width = Math.min(230, node.label.textContent.length * 5.5 + 12);
        labels.push({ node, id, x, y, width, distance, priority: id === this.selected ? 10000 : (this.degree.get(id) || 0) });
      }
    }
    // Keep the selected label, then favor hubs; overlapping labels wait until zoomed in.
    labels.sort((a,b) => b.priority-a.priority || a.distance-b.distance);
    const occupied = [];
    for (const item of labels) {
      const { node, id, x, y, width, distance } = item;
      const box = { left:x-width/2, right:x+width/2, top:y, bottom:y+19 };
      const overlaps = occupied.some(b => box.left < b.right+8 && box.right > b.left-8 && box.top < b.bottom+5 && box.bottom > b.top-5);
      node.label.hidden = overlaps && id !== this.selected;
      if (!node.label.hidden) {
        occupied.push(box);
        node.label.style.transform = `translate(${x}px,${y}px) translateX(-50%)`;
        node.label.style.opacity = String(THREE.MathUtils.clamp(850 / distance, .65, 1));
      }
    }
    this.renderer.render(this.scene, this.camera);
    this.onZoom(Math.round(100 * (this.baseDistance || 600) / this.camera.position.distanceTo(this.controls.target)));
    if (this.steps || changed || animating || this.drag) this.schedule();
  }
  hit(event) {
    const rect = this.renderer.domElement.getBoundingClientRect();
    this.pointer.set((event.clientX - rect.left) / rect.width * 2 - 1, -(event.clientY - rect.top) / rect.height * 2 + 1);
    this.ray.setFromCamera(this.pointer, this.camera);
    return this.ray.intersectObjects([...this.nodes.values()].map(n => n.mesh), false)[0];
  }
  releaseDrag() { this.drag = null; this.controls.enabled = true; this.container.classList.remove('dragging'); }
  bindPointer() {
    const canvas = this.renderer.domElement;
    // Capture phase prevents OrbitControls from orbiting while a sphere is dragged.
    canvas.addEventListener('pointerdown', e => {
      this.fitPending = false;
      if (e.button !== 0 || e.shiftKey || e.ctrlKey || e.metaKey) return;
      const hit = this.hit(e);
      if (!hit) { this.background = { x: e.clientX, y: e.clientY }; return; }
      this.controls.enabled = false;
      this.plane.setFromNormalAndCoplanarPoint(this.camera.getWorldDirection(new THREE.Vector3()), hit.object.position);
      const point = this.ray.ray.intersectPlane(this.plane, new THREE.Vector3());
      if (!point) { this.controls.enabled = true; return; }
      this.drag = { id: hit.object.userData.id, pointer: e.pointerId, x: e.clientX, y: e.clientY, moved: false, offset: hit.object.position.clone().sub(point) };
      canvas.setPointerCapture(e.pointerId); this.container.classList.add('dragging'); e.stopImmediatePropagation();
    }, true);
    canvas.addEventListener('pointermove', e => {
      if (!this.drag) { canvas.style.cursor = this.hit(e) ? 'pointer' : 'grab'; return; }
      if (e.pointerId !== this.drag.pointer) return;
      this.drag.moved ||= Math.hypot(e.clientX-this.drag.x,e.clientY-this.drag.y) > 4;
      if (!this.drag.moved) return;
      this.hit(e);
      const point = this.ray.ray.intersectPlane(this.plane, new THREE.Vector3());
      if (point) {
        point.add(this.drag.offset); const p = this.positions.get(this.drag.id);
        p.x = point.x; p.y = point.y; p.z = point.z; p.vx = p.vy = p.vz = 0;
        this.steps = 0; this.updateGeometry(); this.schedule();
      }
    });
    const end = e => {
      if (this.drag && e.pointerId === this.drag.pointer) {
        if (e.type === 'pointerup' && !this.drag.moved) this.onSelect(this.drag.id);
        if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
        this.releaseDrag(); e.stopImmediatePropagation(); this.schedule();
      } else if (this.background) {
        if (e.type === 'pointerup' && Math.hypot(e.clientX-this.background.x,e.clientY-this.background.y) < 4) this.onSelect(null);
        this.background = null;
      }
    };
    canvas.addEventListener('pointerup', end, true); canvas.addEventListener('pointercancel', end, true);
    canvas.addEventListener('wheel', () => { this.fitPending = false; }, { passive: true });
  }
}
