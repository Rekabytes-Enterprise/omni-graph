//! Session-local graph layout and camera. Moving nodes never changes memory data.
use crate::model::{Edge, Graph, Kind, Memory, Scope};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Paragraph,
        canvas::{Canvas, Circle, Line as GraphLine, Points},
    },
};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

pub(super) struct Scene {
    pub positions: HashMap<String, Point>,
    degrees: HashMap<String, usize>,
    pub center: Point,
    pub zoom: f64,
    pub labels: bool,
    targets: HashMap<String, Point>,
    pulse: Option<f64>,
    flowing: bool,
    flow_phase: f64,
}
impl Scene {
    pub fn new(graph: &Graph) -> Self {
        Self {
            positions: layout(graph),
            degrees: degrees(graph),
            center: Point::default(),
            zoom: 0.85,
            labels: true,
            targets: HashMap::new(),
            pulse: None,
            flowing: false,
            flow_phase: 0.0,
        }
    }
    pub fn sync(&mut self, graph: &Graph) {
        self.degrees = degrees(graph);
        if self.positions.len() == graph.memories.len()
            && graph
                .memories
                .iter()
                .all(|m| self.positions.contains_key(&m.id))
        {
            return;
        }
        let fresh = layout(graph);
        self.positions.retain(|id, _| fresh.contains_key(id));
        self.targets.retain(|id, _| fresh.contains_key(id));
        let existing = self.positions.clone();
        for (id, point) in fresh {
            if !self.positions.contains_key(&id) {
                // New memories emerge near existing neighbors, then settle into their layout.
                let linked: Vec<_> = graph
                    .edges
                    .iter()
                    .filter_map(|edge| {
                        let other = if edge.from == id {
                            &edge.to
                        } else if edge.to == id {
                            &edge.from
                        } else {
                            return None;
                        };
                        existing.get(other).copied()
                    })
                    .collect();
                let origin = if linked.is_empty() {
                    point
                } else {
                    Point {
                        x: linked.iter().map(|p| p.x).sum::<f64>() / linked.len() as f64,
                        y: linked.iter().map(|p| p.y).sum::<f64>() / linked.len() as f64,
                    }
                };
                self.positions.insert(id.clone(), origin);
                self.targets.insert(id, point);
            }
        }
    }
    pub fn pulse(&mut self) {
        self.pulse = Some(0.0);
    }
    pub fn is_animating(&self) -> bool {
        self.flowing || self.pulse.is_some() || !self.targets.is_empty()
    }
    pub fn set_flowing(&mut self, enabled: bool) {
        self.flowing = enabled;
    }
    fn flows_on(&self, edge: &Edge, selected: Option<&str>) -> bool {
        self.flowing
            && edge.from != edge.to
            && (Some(edge.from.as_str()) == selected || Some(edge.to.as_str()) == selected)
    }
    fn flow_position(&self, edge: &Edge) -> f64 {
        let offset = edge
            .from
            .bytes()
            .chain(edge.to.bytes())
            .fold(0_u32, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(byte as u32)
            });
        (self.flow_phase / 1.8 + (offset % 100) as f64 / 100.0).fract()
    }
    pub fn relayout(&mut self, graph: &Graph) {
        self.targets = layout(graph);
        self.pulse();
    }
    pub fn stop_node(&mut self, id: &str) {
        self.targets.remove(id);
    }
    pub fn animate(&mut self, elapsed: Duration) -> bool {
        let active = self.is_animating();
        if self.flowing {
            self.flow_phase = (self.flow_phase + elapsed.as_secs_f64()) % 1.8;
        }
        if let Some(age) = &mut self.pulse {
            *age += elapsed.as_secs_f64();
            if *age >= 0.7 {
                self.pulse = None;
            }
        }
        let blend = 1.0 - (-elapsed.as_secs_f64() / 0.09).exp();
        self.targets.retain(|id, target| {
            let Some(point) = self.positions.get_mut(id) else {
                return false;
            };
            point.x += (target.x - point.x) * blend;
            point.y += (target.y - point.y) * blend;
            if point.distance(*target) * self.zoom < 0.02 {
                *point = *target;
                false
            } else {
                true
            }
        });
        active
    }
    pub fn center_on(&mut self, id: &str) {
        if let Some(point) = self.positions.get(id) {
            self.center = *point;
        }
    }
    pub fn fit<'a>(&mut self, ids: impl Iterator<Item = &'a str>, area: Rect) {
        let points: Vec<_> = ids.filter_map(|id| self.positions.get(id)).collect();
        if points.is_empty() || area.width < 2 || area.height < 2 {
            return;
        }
        let mut min = Point {
            x: f64::INFINITY,
            y: f64::INFINITY,
        };
        let mut max = Point {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
        };
        for p in points {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        self.center = Point {
            x: (min.x + max.x) / 2.0,
            y: (min.y + max.y) / 2.0,
        };
        self.zoom = ((area.width.saturating_sub(1) as f64 / (max.x - min.x + 24.0))
            .min(2.0 * area.height.saturating_sub(1) as f64 / (max.y - min.y + 24.0)))
        .clamp(0.005, 1.2);
    }
    pub fn bounds(&self, area: Rect) -> ([f64; 2], [f64; 2]) {
        let half_x = area.width.saturating_sub(1).max(1) as f64 / (2.0 * self.zoom);
        // Terminal cells are approximately twice as tall as they are wide.
        let half_y = area.height.saturating_sub(1).max(1) as f64 / self.zoom;
        (
            [self.center.x - half_x, self.center.x + half_x],
            [self.center.y - half_y, self.center.y + half_y],
        )
    }
    pub fn screen_to_world(&self, screen: Point, area: Rect) -> Point {
        Point {
            x: self.center.x
                + (screen.x - area.x as f64 - area.width.saturating_sub(1) as f64 / 2.0)
                    / self.zoom,
            y: self.center.y
                - 2.0 * (screen.y - area.y as f64 - area.height.saturating_sub(1) as f64 / 2.0)
                    / self.zoom,
        }
    }
    pub fn world_to_screen(&self, world: Point, area: Rect) -> Point {
        Point {
            x: area.x as f64
                + area.width.saturating_sub(1) as f64 / 2.0
                + (world.x - self.center.x) * self.zoom,
            y: area.y as f64 + area.height.saturating_sub(1) as f64 / 2.0
                - (world.y - self.center.y) * self.zoom / 2.0,
        }
    }
    pub fn pan_cells(&mut self, dx: f64, dy: f64) {
        self.center.x += dx / self.zoom;
        self.center.y -= 2.0 * dy / self.zoom;
    }
    pub fn zoom_at(&mut self, factor: f64, screen: Point, area: Rect) {
        let before = self.screen_to_world(screen, area);
        self.zoom = (self.zoom * factor).clamp(0.005, 4.0);
        let after = self.screen_to_world(screen, area);
        self.center.x += before.x - after.x;
        self.center.y += before.y - after.y;
    }
    pub(super) fn ring_radius(memory: &Memory, selected: bool) -> f64 {
        if selected {
            4.8
        } else if memory.data.kind == Kind::Component {
            4.2
        } else {
            3.3
        }
    }
    pub(super) fn body_radius(&self, memory: &Memory) -> f64 {
        let connections = self.degrees.get(&memory.id).copied().unwrap_or(0);
        let bonus = ((connections.saturating_sub(1) as f64).ln_1p() * 0.32).min(1.0);
        // Grow the solid center, keeping the surrounding ring at its original size.
        let world = 1.0 + bonus;
        // A subcell minimum keeps filled centers visible at intermediate zoom levels.
        world.max((0.72 + 0.28 * bonus) / self.zoom)
    }

    pub(super) fn radius(&self, memory: &Memory, selected: bool) -> f64 {
        let ring = if self.zoom >= 0.28 || selected {
            Self::ring_radius(memory, selected)
        } else {
            0.0
        };
        self.body_radius(memory).max(ring)
    }
    pub fn hit<'a>(
        &self,
        nodes: impl Iterator<Item = &'a Memory>,
        screen: Point,
        area: Rect,
        selected: Option<&str>,
    ) -> Option<String> {
        let world = self.screen_to_world(screen, area);
        nodes
            .filter_map(|m| self.positions.get(&m.id).map(|p| (m, p.distance(world))))
            .filter(|(m, d)| {
                *d <= self.radius(m, Some(m.id.as_str()) == selected) + 1.2 / self.zoom
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(m, _)| m.id.clone())
    }
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        nodes: &[&Memory],
        edges: &[Edge],
        selected: Option<&str>,
        edge_target: Option<&str>,
    ) -> Paint {
        let ids: HashSet<_> = nodes.iter().map(|m| m.id.as_str()).collect();
        let mut edges: Vec<_> = edges
            .iter()
            .filter(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()))
            .collect();
        // Paint the selected neighborhood last so crossing background links cannot dim it.
        edges.sort_by_key(|edge| {
            let incident =
                Some(edge.from.as_str()) == selected || Some(edge.to.as_str()) == selected;
            let target =
                Some(edge.from.as_str()) == edge_target || Some(edge.to.as_str()) == edge_target;
            (incident, incident && target)
        });
        let radii: HashMap<_, _> = nodes
            .iter()
            .map(|m| {
                (
                    m.id.as_str(),
                    self.radius(m, Some(m.id.as_str()) == selected),
                )
            })
            .collect();
        let neighbors: HashSet<_> = edges
            .iter()
            .filter_map(|e| {
                if Some(e.from.as_str()) == selected {
                    Some(e.to.as_str())
                } else if Some(e.to.as_str()) == selected {
                    Some(e.from.as_str())
                } else {
                    None
                }
            })
            .collect();
        let (xb, yb) = self.bounds(area);
        let canvas = Canvas::default()
            .background_color(super::BACKGROUND)
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(|ctx| {
                for edge in &edges {
                    let (Some(from), Some(to)) =
                        (self.positions.get(&edge.from), self.positions.get(&edge.to))
                    else {
                        continue;
                    };
                    let incident =
                        Some(edge.from.as_str()) == selected || Some(edge.to.as_str()) == selected;
                    let highlighted = incident
                        && (Some(edge.from.as_str()) == edge_target
                            || Some(edge.to.as_str()) == edge_target);
                    let color = if highlighted {
                        Color::Yellow
                    } else if incident {
                        Color::Cyan
                    } else {
                        Color::Rgb(62, 74, 90)
                    };
                    let path = EdgePath::new(
                        *from,
                        *to,
                        radii[edge.from.as_str()],
                        radii[edge.to.as_str()],
                        self.zoom,
                    );
                    let mut previous = path.at(0.0);
                    // Sample at Braille subcell resolution; cap work for very long offscreen links.
                    let steps =
                        ((from.distance(*to) * self.zoom * 2.5).ceil() as usize).clamp(8, 512);
                    for step in 1..=steps {
                        let next = path.at(step as f64 / steps as f64);
                        ctx.draw(&GraphLine {
                            x1: previous.x,
                            y1: previous.y,
                            x2: next.x,
                            y2: next.y,
                            color,
                        });
                        previous = next;
                    }
                    if self.flows_on(edge, selected) {
                        let point = path.at(self.flow_position(edge));
                        ctx.draw(&Circle::new(
                            point.x,
                            point.y,
                            0.5 / self.zoom,
                            Color::White,
                        ));
                    }
                    if incident && self.zoom >= 0.35 {
                        let tip = path.at(1.0);
                        let tangent = path.tangent(1.0);
                        for side in [-1.0, 1.0] {
                            ctx.draw(&GraphLine {
                                x1: tip.x,
                                y1: tip.y,
                                x2: tip.x - 2.8 * tangent.x + 1.6 * side * tangent.y,
                                y2: tip.y - 2.8 * tangent.y - 1.6 * side * tangent.x,
                                color,
                            });
                        }
                    }
                }
                for memory in nodes {
                    let Some(point) = self.positions.get(&memory.id) else {
                        continue;
                    };
                    let is_selected = Some(memory.id.as_str()) == selected;
                    let color =
                        node_color(memory, is_selected, Some(memory.id.as_str()) == edge_target);
                    let r = self.radius(memory, is_selected);
                    if point.x + r < xb[0]
                        || point.x - r > xb[1]
                        || point.y + r < yb[0]
                        || point.y - r > yb[1]
                    {
                        continue;
                    }
                    let ring = Self::ring_radius(memory, is_selected);
                    if self.zoom >= 0.28 || is_selected {
                        ctx.draw(&Circle::new(point.x, point.y, ring, color));
                        if is_selected {
                            ctx.draw(&Circle::new(point.x, point.y, ring + 0.65, color));
                        }
                    }

                    if is_selected && let Some(age) = self.pulse {
                        let phase = age / 0.7;
                        let intensity = 1.0 - phase;
                        let color = Color::Rgb(
                            (35.0 + 50.0 * intensity) as u8,
                            (37.0 + 150.0 * intensity) as u8,
                            (42.0 + 160.0 * intensity) as u8,
                        );
                        ctx.draw(&Circle::new(
                            point.x,
                            point.y,
                            ring.max(0.8 / self.zoom) + (0.3 + phase * 0.8) / self.zoom,
                            color,
                        ));
                    }
                    if is_selected || self.zoom >= 0.65 {
                        ctx.draw(&Points {
                            coords: &[(point.x, point.y)],
                            color,
                        });
                    }
                }
            });
        frame.render_widget(canvas, area);
        // Filled bodies use half-block pixels, rather than dotted Braille outlines.
        for memory in nodes {
            let Some(point) = self.positions.get(&memory.id) else {
                continue;
            };
            let screen = self.world_to_screen(*point, area);
            let active = Some(memory.id.as_str()) == selected;
            let color = node_color(memory, active, Some(memory.id.as_str()) == edge_target);
            if self.zoom < 0.65 {
                let (x, y) = (screen.x.round(), screen.y.round());
                if x >= area.x as f64
                    && x < area.right() as f64
                    && y >= area.y as f64
                    && y < area.bottom() as f64
                {
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            if self.degrees.get(&memory.id).copied().unwrap_or(0) >= 4 {
                                "⬤"
                            } else {
                                "●"
                            },
                            Style::default().fg(color),
                        )),
                        Rect::new(x as u16, y as u16, 1, 1),
                    );
                }
            } else {
                let radius = self.body_radius(memory) * self.zoom;
                let left = (screen.x - radius).floor().max(area.x as f64) as u16;
                let right = (screen.x + radius)
                    .ceil()
                    .min(area.right().saturating_sub(1) as f64) as u16;
                let top = (screen.y - radius / 2.0).floor().max(area.y as f64) as u16;
                let bottom = (screen.y + radius / 2.0)
                    .ceil()
                    .min(area.bottom().saturating_sub(1) as f64)
                    as u16;
                for y in top..=bottom {
                    for x in left..=right {
                        let inside = |offset: f64| {
                            (x as f64 - screen.x).hypot(2.0 * (y as f64 + offset - screen.y))
                                <= radius
                        };
                        let symbol = match (inside(-0.25), inside(0.25)) {
                            (true, true) => "█",
                            (true, false) => "▀",
                            (false, true) => "▄",
                            (false, false) => continue,
                        };
                        frame.render_widget(
                            Paragraph::new(symbol)
                                .style(Style::default().fg(color).bg(super::BACKGROUND)),
                            Rect::new(x, y, 1, 1),
                        );
                    }
                }
            }
        }
        let mut labels = vec![];
        let mut on_screen = 0;
        let mut ordered: Vec<_> = nodes
            .iter()
            .filter_map(|memory| self.positions.get(&memory.id).map(|p| (*memory, *p)))
            .collect();
        ordered.sort_by_key(|(m, _)| {
            (
                Some(m.id.as_str()) != selected,
                !neighbors.contains(m.id.as_str()),
                m.data.kind != Kind::Component,
                &m.id,
            )
        });
        let mut occupied = vec![];
        // Reserve node centers so a nearby label never covers another circle.
        for (memory, point) in &ordered {
            let screen = self.world_to_screen(*point, area);
            if screen.x >= area.x as f64
                && screen.x < area.right() as f64
                && screen.y >= area.y as f64
                && screen.y < area.bottom() as f64
            {
                on_screen += 1;
                let rx = (self.radius(memory, Some(memory.id.as_str()) == selected) * self.zoom)
                    .ceil()
                    .max(1.0) as u16;
                let ry = (self.radius(memory, Some(memory.id.as_str()) == selected) * self.zoom
                    / 2.0)
                    .ceil()
                    .max(1.0) as u16;
                occupied.push(Rect::new(
                    (screen.x.round() as u16).saturating_sub(rx),
                    (screen.y.round() as u16).saturating_sub(ry),
                    rx.saturating_mul(2).saturating_add(1),
                    ry.saturating_mul(2).saturating_add(1),
                ));
            }
        }
        for (memory, point) in ordered {
            let active = Some(memory.id.as_str()) == selected;
            if !active
                && (!self.labels
                    || (self.zoom < 0.6
                        && !neighbors.contains(memory.id.as_str())
                        && memory.data.kind != Kind::Component))
            {
                continue;
            }
            let screen = self.world_to_screen(point, area);
            if screen.x < area.x as f64
                || screen.x >= area.right() as f64
                || screen.y < area.y as f64
                || screen.y >= area.bottom() as f64
            {
                continue;
            }
            let text = truncate(
                &memory.data.title,
                if active {
                    32
                } else if self.zoom < 0.65 {
                    16
                } else {
                    24
                },
            );
            let width = Line::from(text.as_str()).width().min(area.width as usize) as u16;
            let rx = (self.radius(memory, active) * self.zoom).ceil() as i32;
            let ry = (self.radius(memory, active) * self.zoom / 2.0).ceil() as i32;
            let (cx, cy) = (screen.x.round() as i32, screen.y.round() as i32);
            let candidates = [
                (cx - width as i32 / 2, cy + ry + 1),
                (cx + rx + 1, cy),
                (cx - width as i32 / 2, cy - ry - 1),
                (cx - rx - 1 - width as i32, cy),
            ];
            for (x, y) in candidates {
                if x < area.x as i32
                    || y < area.y as i32
                    || x + width as i32 > area.right() as i32
                    || y >= area.bottom() as i32
                {
                    continue;
                }
                let rect = Rect::new(x as u16, y as u16, width, 1);
                if occupied.iter().any(|r| r.intersects(rect)) {
                    continue;
                }
                let color = node_color(memory, active, Some(memory.id.as_str()) == edge_target);

                frame.render_widget(
                    Paragraph::new(Span::styled(
                        text.clone(),
                        Style::default().fg(color).add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                    ))
                    .style(Style::default().bg(super::BACKGROUND)),
                    rect,
                );
                occupied.push(rect);
                labels.push((rect, memory.id.clone()));
                break;
            }
        }
        Paint {
            labels,
            on_screen,
            edges: edges.len(),
        }
    }
}
pub(super) struct Paint {
    pub labels: Vec<(Rect, String)>,
    pub on_screen: usize,
    pub edges: usize,
}
// Quadratic curves meet each node along their endpoint tangent.
struct EdgePath {
    start: Point,
    control: Point,
    end: Point,
}
impl EdgePath {
    fn new(from: Point, to: Point, from_radius: f64, to_radius: f64, zoom: f64) -> Self {
        let distance = from.distance(to).max(0.001);
        let bend = (distance * 0.09).min(6.0 / zoom);
        let control = Point {
            x: (from.x + to.x) / 2.0 - (to.y - from.y) / distance * bend,
            y: (from.y + to.y) / 2.0 + (to.x - from.x) / distance * bend,
        };
        let inset = |center: Point, radius: f64| {
            let length = center.distance(control).max(0.001);
            let amount = radius.min(distance / 3.0) / length;
            Point {
                x: center.x + (control.x - center.x) * amount,
                y: center.y + (control.y - center.y) * amount,
            }
        };
        Self {
            start: inset(from, from_radius),
            control,
            end: inset(to, to_radius),
        }
    }
    fn at(&self, t: f64) -> Point {
        let u = 1.0 - t;
        Point {
            x: u * u * self.start.x + 2.0 * u * t * self.control.x + t * t * self.end.x,
            y: u * u * self.start.y + 2.0 * u * t * self.control.y + t * t * self.end.y,
        }
    }
    fn tangent(&self, t: f64) -> Point {
        let x = (1.0 - t) * (self.control.x - self.start.x) + t * (self.end.x - self.control.x);
        let y = (1.0 - t) * (self.control.y - self.start.y) + t * (self.end.y - self.control.y);
        let length = x.hypot(y).max(0.001);
        Point {
            x: x / length,
            y: y / length,
        }
    }
}
fn degrees(graph: &Graph) -> HashMap<String, usize> {
    let mut neighbors: HashMap<&str, HashSet<&str>> = HashMap::new();
    for edge in &graph.edges {
        if edge.from != edge.to {
            neighbors.entry(&edge.from).or_default().insert(&edge.to);
            neighbors.entry(&edge.to).or_default().insert(&edge.from);
        }
    }
    neighbors
        .into_iter()
        .map(|(id, linked)| (id.to_owned(), linked.len()))
        .collect()
}
fn node_color(memory: &Memory, selected: bool, target: bool) -> Color {
    if selected {
        return Color::Cyan;
    }
    if target {
        return Color::Yellow;
    }
    match memory.data.kind {
        Kind::Mistake | Kind::Issue => Color::LightRed,
        Kind::Fix => Color::LightGreen,
        Kind::Lesson => Color::LightBlue,
        Kind::Component => Color::White,
        _ => match memory.data.scope {
            Scope::Global => Color::Magenta,
            Scope::Client => Color::Blue,
            Scope::Task => Color::Yellow,
            Scope::Project => Color::Gray,
        },
    }
}
fn truncate(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return text.to_owned();
    }
    let mut output = String::new();
    for c in text.chars() {
        let next = format!("{output}{c}…");
        if Line::from(next.as_str()).width() > width {
            break;
        }
        output.push(c);
    }
    output.push('…');
    output
}
/// Bounded, deterministic force layout, computed only when the node set changes.
/// All nodes are available to the camera; the UI no longer pages six neighbors.
fn layout(graph: &Graph) -> HashMap<String, Point> {
    let mut degrees: HashMap<&str, usize> = HashMap::new();
    for edge in &graph.edges {
        *degrees.entry(&edge.from).or_default() += 1;
        *degrees.entry(&edge.to).or_default() += 1;
    }
    let mut ids: Vec<_> = graph.memories.iter().map(|m| m.id.as_str()).collect();
    ids.sort_by(|a, b| {
        degrees
            .get(b)
            .unwrap_or(&0)
            .cmp(degrees.get(a).unwrap_or(&0))
            .then(a.cmp(b))
    });
    let indices: HashMap<_, _> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|e| Some((*indices.get(e.from.as_str())?, *indices.get(e.to.as_str())?)))
        .collect();
    let mut points: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let angle = i as f64 * 2.399_963_229_728_653;
            let radius = 22.0 * (i as f64).sqrt();
            Point {
                x: radius * angle.cos(),
                y: radius * angle.sin(),
            }
        })
        .collect();
    // Exact repulsion is enough for the demo; sampling caps work on larger stores.
    let iterations = if ids.len() <= 300 { 180 } else { 70 };
    let stride = (ids.len() / 300).max(1);
    for iteration in 0..iterations {
        let mut forces = vec![Point::default(); ids.len()];
        for i in 0..points.len() {
            for j in ((i + 1)..points.len()).step_by(stride) {
                let dx = points[i].x - points[j].x;
                let dy = points[i].y - points[j].y;
                let distance = dx.hypot(dy).max(0.5);
                let strength = 80.0 / (distance * distance);
                let (fx, fy) = (dx * strength, dy * strength);
                forces[i].x += fx;
                forces[i].y += fy;
                forces[j].x -= fx;
                forces[j].y -= fy;
            }
        }
        for &(from, to) in &edges {
            let dx = points[to].x - points[from].x;
            let dy = points[to].y - points[from].y;
            let distance = dx.hypot(dy).max(0.5);
            let strength = (distance - 24.0) * 0.16 / distance;
            forces[from].x += dx * strength;
            forces[from].y += dy * strength;
            forces[to].x -= dx * strength;
            forces[to].y -= dy * strength;
        }
        let cooling = 5.0 * (1.0 - iteration as f64 / iterations as f64) + 0.15;
        for (point, force) in points.iter_mut().zip(forces) {
            let fx = force.x - point.x * 0.006;
            let fy = force.y - point.y * 0.006;
            let length = fx.hypot(fy).max(0.01);
            let step = cooling.min(length) / length;
            point.x += fx * step;
            point.y += fy * step;
        }
    }
    ids.into_iter()
        .zip(points)
        .map(|(id, p)| (id.to_owned(), p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_keeps_the_pointer_on_the_same_world_point_and_is_bounded() {
        let mut scene = Scene::new(&Graph::default());
        scene.center = Point { x: 31.0, y: -10.0 };
        let area = Rect::new(40, 5, 100, 30);
        let cursor = Point { x: 115.0, y: 13.0 };
        let before = scene.screen_to_world(cursor, area);
        scene.zoom_at(1.7, cursor, area);
        assert!(scene.screen_to_world(cursor, area).distance(before) < 1e-9);
        for _ in 0..100 {
            scene.zoom_at(2.0, cursor, area);
        }
        assert_eq!(scene.zoom, 4.0);
        for _ in 0..100 {
            scene.zoom_at(0.5, cursor, area);
        }
        assert_eq!(scene.zoom, 0.005);
    }
    #[test]
    fn flow_repeats_with_bounded_time_and_pauses_cleanly() {
        let mut scene = Scene::new(&Graph::default());
        let edge = Edge {
            from: "a".into(),
            to: "b".into(),
            relation: crate::model::Relation::RelatedTo,
        };
        scene.set_flowing(true);
        let start = scene.flow_position(&edge);
        scene.animate(Duration::from_millis(500));
        assert_ne!(start, scene.flow_position(&edge));
        scene.animate(Duration::from_millis(1300));
        assert!((start - scene.flow_position(&edge)).abs() < 1e-9);
        scene.animate(Duration::from_secs(60));
        assert!(scene.is_animating() && scene.flow_phase < 1.8);
        scene.set_flowing(false);
        let phase = scene.flow_phase;
        assert!(!scene.animate(Duration::from_secs(2)));
        assert_eq!(scene.flow_phase, phase);
        assert!(!scene.is_animating());
    }
    #[test]
    fn every_incident_connection_flows_while_other_edges_stay_static() {
        let mut scene = Scene::new(&Graph::default());
        let edges: Vec<_> = [
            ("hub", "a"),
            ("b", "hub"),
            ("hub", "c"),
            ("a", "b"),
            ("hub", "hub"),
        ]
        .into_iter()
        .map(|(from, to)| Edge {
            from: from.into(),
            to: to.into(),
            relation: crate::model::Relation::RelatedTo,
        })
        .collect();
        scene.set_flowing(true);
        let phases: Vec<_> = edges.iter().map(|edge| scene.flow_position(edge)).collect();
        assert_ne!(phases[0], phases[1]);
        assert_eq!(
            edges
                .iter()
                .map(|edge| scene.flows_on(edge, Some("hub")))
                .collect::<Vec<_>>(),
            [true, true, true, false, false]
        );
        assert!(edges.iter().all(|edge| !scene.flows_on(edge, None)));
        scene.set_flowing(false);
        assert!(edges.iter().all(|edge| !scene.flows_on(edge, Some("hub"))));
    }
    #[test]
    fn curved_edges_meet_node_boundaries_and_have_finite_tangents() {
        let from = Point { x: 0.0, y: 0.0 };
        let to = Point { x: 100.0, y: 0.0 };
        for zoom in [0.005, 0.06, 0.85, 4.0] {
            let path = EdgePath::new(from, to, 5.0, 7.0, zoom);
            assert!((path.at(0.0).distance(from) - 5.0).abs() < 1e-9);
            assert!((path.at(1.0).distance(to) - 7.0).abs() < 1e-9);
            assert!(path.at(0.5).y > 0.0);
            assert!(path.at(0.5).y * zoom <= 6.0);
            for i in 0..=100 {
                let t = i as f64 / 100.0;
                let tangent = path.tangent(t);
                assert!((tangent.x.hypot(tangent.y) - 1.0).abs() < 1e-9);
                assert!(path.at(t).x.is_finite() && path.at(t).y.is_finite());
            }
            let reverse = EdgePath::new(to, from, 7.0, 5.0, zoom);
            assert!(reverse.at(0.5).y < 0.0);
        }
        let coincident = EdgePath::new(from, from, 5.0, 5.0, 1.0);
        assert!(coincident.at(0.5).x.is_finite());
        assert!(coincident.tangent(1.0).y.is_finite());
    }
    #[test]
    fn projection_roundtrips_and_preserves_circle_aspect() {
        let scene = Scene::new(&Graph::default());
        let area = Rect::new(5, 7, 90, 25);
        let world = Point { x: 27.0, y: -12.0 };
        assert!(
            scene
                .screen_to_world(scene.world_to_screen(world, area), area)
                .distance(world)
                < 1e-9
        );
        let origin = scene.world_to_screen(Point::default(), area);
        let right = scene.world_to_screen(Point { x: 10.0, y: 0.0 }, area);
        let up = scene.world_to_screen(Point { x: 0.0, y: 10.0 }, area);
        assert!(((right.x - origin.x) - 2.0 * (origin.y - up.y)).abs() < 1e-9);
    }
}
