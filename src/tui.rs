mod graph;

use crate::{model::*, storage::Workspace};
use anyhow::{Result, ensure};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    style::Print,
};
use graph::{Point, Scene};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{
    collections::HashSet,
    io::{self, IsTerminal},
    time::{Duration, Instant},
};

const BACKGROUND: Color = Color::Rgb(35, 37, 42);
const MUTED: Color = Color::Rgb(145, 154, 168);
const SELECTION: Color = Color::Rgb(62, 69, 80);

#[derive(Clone, Copy)]
struct Camera {
    center: Point,
    zoom: f64,
}

struct App {
    graph: Graph,
    scene: Scene,
    motion: Option<Camera>,
    areas: Areas,
    drag: Option<Drag>,
    graph_focus: bool,
    expanded: bool,
    mouse_enabled: bool,
    flow_enabled: bool,
    query: String,
    search_matches: Option<HashSet<String>>,
    typing: bool,
    selected: usize,
    neighbor: usize,
    history: Vec<String>,
    all: bool,
    scope: Option<Scope>,
    scroll: u16,
}
#[derive(Default)]
struct Areas {
    list: Rect,
    details: Rect,
    graph: Rect,
    list_offset: usize,
    labels: Vec<(Rect, String)>,
}
enum Drag {
    Node { id: String, offset: Point },
    Pan { last: Point, button: MouseButton },
}
fn contains(area: Rect, point: Point) -> bool {
    point.x >= area.x as f64
        && point.x < area.right() as f64
        && point.y >= area.y as f64
        && point.y < area.bottom() as f64
}
impl App {
    fn new(graph: Graph) -> Self {
        let scene = Scene::new(&graph);
        let mut app = Self {
            graph,
            scene,
            motion: None,
            areas: Areas::default(),
            drag: None,
            graph_focus: true,
            expanded: false,
            mouse_enabled: true,
            flow_enabled: true,
            query: String::new(),
            search_matches: None,
            typing: false,
            selected: 0,
            neighbor: 0,
            history: vec![],
            all: false,
            scope: None,
            scroll: 0,
        };
        app.focus_current();
        app
    }
    fn focus_current(&mut self) {
        self.motion = None;
        if let Some(id) = self.current().map(|m| m.id.clone()) {
            self.scene.pulse();
            self.scene.center_on(&id);
        }
    }
    fn move_selection(&mut self, offset: isize) {
        let count = self.visible().len();
        self.selected = self
            .selected
            .saturating_add_signed(offset)
            .min(count.saturating_sub(1));
        self.neighbor = 0;
        self.scroll = 0;
        self.focus_current();
    }
    fn pick(&mut self, id: &str) {
        if let Some(index) = self.visible().iter().position(|m| m.id == id) {
            if let Some(current) = self.current().filter(|m| m.id != id) {
                self.history.push(current.id.clone());
            }
            self.scene.pulse();
            self.selected = index;
            self.neighbor = 0;
            self.scroll = 0;
        }
    }
    fn fit(&mut self) {
        self.motion = None;
        let ids: Vec<_> = self.visible().iter().map(|m| m.id.clone()).collect();
        self.scene
            .fit(ids.iter().map(String::as_str), self.areas.graph);
    }
    fn zoom(&mut self, factor: f64) {
        let area = self.areas.graph;
        let cursor = Point {
            x: area.x as f64 + area.width.saturating_sub(1) as f64 / 2.0,
            y: area.y as f64 + area.height.saturating_sub(1) as f64 / 2.0,
        };
        self.scene.zoom_at(factor, cursor, area);
    }
    // Accumulate wheel input against the destination, avoiding a queue of animations.
    fn wheel(&mut self, point: Point, factor: f64, dx: f64, dy: f64) {
        if self.drag.is_some() {
            return;
        }
        let current = Camera {
            center: self.scene.center,
            zoom: self.scene.zoom,
        };
        if let Some(target) = self.motion {
            self.scene.center = target.center;
            self.scene.zoom = target.zoom;
        }
        self.scene.zoom_at(factor, point, self.areas.graph);
        self.scene.pan_cells(dx, dy);
        self.motion = Some(Camera {
            center: self.scene.center,
            zoom: self.scene.zoom,
        });
        self.scene.center = current.center;
        self.scene.zoom = current.zoom;
    }
    fn update_flow(&mut self) {
        let visible = self.visible();
        let current = self.current().map(|m| m.id.as_str());
        let ids: HashSet<_> = visible.iter().map(|m| m.id.as_str()).collect();
        let connected = current.is_some()
            && self.graph.edges.iter().any(|edge| {
                (Some(edge.from.as_str()) == current || Some(edge.to.as_str()) == current)
                    && edge.from != edge.to
                    && ids.contains(edge.from.as_str())
                    && ids.contains(edge.to.as_str())
            });
        self.scene
            .set_flowing(self.flow_enabled && !self.typing && connected);
    }
    fn animating(&self) -> bool {
        self.motion.is_some() || self.scene.is_animating()
    }
    fn animate(&mut self, elapsed: Duration) -> bool {
        let scene_changed = self.scene.animate(elapsed);
        let Some(target) = self.motion else {
            return scene_changed;
        };
        let blend = 1.0 - (-elapsed.as_secs_f64() / 0.045).exp();
        // Interpolating inverse zoom with position preserves the cursor's world anchor.
        self.scene.zoom =
            1.0 / (1.0 / self.scene.zoom + (1.0 / target.zoom - 1.0 / self.scene.zoom) * blend);
        self.scene.center.x += (target.center.x - self.scene.center.x) * blend;
        self.scene.center.y += (target.center.y - self.scene.center.y) * blend;
        if (self.scene.zoom / target.zoom - 1.0).abs() < 0.0001
            && (self.scene.center.x - target.center.x).hypot(self.scene.center.y - target.center.y)
                * self.scene.zoom
                < 0.01
        {
            self.scene.center = target.center;
            self.scene.zoom = target.zoom;
            self.motion = None;
        }
        true
    }
    fn mouse(&mut self, event: MouseEvent) -> bool {
        let point = Point {
            x: event.column as f64,
            y: event.row as f64,
        };
        match event.kind {
            MouseEventKind::Down(button) if contains(self.areas.graph, point) => {
                self.motion = None;
                self.graph_focus = true;
                let node = if button == MouseButton::Left {
                    self.areas
                        .labels
                        .iter()
                        .find(|(rect, _)| contains(*rect, point))
                        .map(|(_, id)| id.clone())
                        .or_else(|| {
                            self.scene.hit(
                                self.visible().into_iter(),
                                point,
                                self.areas.graph,
                                self.current().map(|m| m.id.as_str()),
                            )
                        })
                } else {
                    None
                };
                if let Some(id) = node {
                    self.pick(&id);
                    self.scene.stop_node(&id);
                    let world = self.scene.screen_to_world(point, self.areas.graph);
                    if let Some(position) = self.scene.positions.get(&id) {
                        self.drag = Some(Drag::Node {
                            id,
                            offset: Point {
                                x: world.x - position.x,
                                y: world.y - position.y,
                            },
                        });
                    }
                } else {
                    self.drag = Some(Drag::Pan {
                        last: point,
                        button,
                    });
                }
                true
            }
            MouseEventKind::Down(MouseButton::Left) if contains(self.areas.list, point) => {
                self.graph_focus = false;
                let index = self.areas.list_offset + (event.row - self.areas.list.y) as usize / 2;
                if let Some(id) = self.visible().get(index).map(|m| m.id.clone()) {
                    self.pick(&id);
                    self.focus_current();
                }
                true
            }
            MouseEventKind::Drag(button) => match &mut self.drag {
                Some(Drag::Node { id, offset }) if button == MouseButton::Left => {
                    let world = self.scene.screen_to_world(point, self.areas.graph);
                    self.scene.positions.insert(
                        id.clone(),
                        Point {
                            x: world.x - offset.x,
                            y: world.y - offset.y,
                        },
                    );
                    true
                }
                Some(Drag::Pan {
                    last,
                    button: started,
                }) if button == *started => {
                    self.scene.pan_cells(last.x - point.x, last.y - point.y);
                    *last = point;
                    true
                }
                _ => false,
            },
            MouseEventKind::Up(_) => self.drag.take().is_some(),
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = event.kind == MouseEventKind::ScrollUp;
                if contains(self.areas.graph, point) {
                    if event.modifiers.contains(KeyModifiers::SHIFT) {
                        self.wheel(point, 1.0, 0.0, if up { -1.5 } else { 1.5 });
                    } else {
                        self.wheel(point, if up { 1.06 } else { 1.0 / 1.06 }, 0.0, 0.0);
                    }
                } else if contains(self.areas.list, point) {
                    self.move_selection(if up { -1 } else { 1 });
                } else if contains(self.areas.details, point) {
                    self.scroll = if up {
                        self.scroll.saturating_sub(3)
                    } else {
                        self.scroll.saturating_add(3)
                    };
                } else {
                    return false;
                }
                true
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight
                if contains(self.areas.graph, point) =>
            {
                self.wheel(
                    point,
                    1.0,
                    if event.kind == MouseEventKind::ScrollLeft {
                        -2.0
                    } else {
                        2.0
                    },
                    0.0,
                );
                true
            }
            _ => false,
        }
    }

    fn visible(&self) -> Vec<&Memory> {
        let query = self.query.trim();
        let mut nodes: Vec<_> = self
            .graph
            .memories
            .iter()
            .filter(|m| {
                (self.all || m.active())
                    && self.scope.is_none_or(|s| s == m.data.scope)
                    && (query.is_empty()
                        || self
                            .search_matches
                            .as_ref()
                            .is_some_and(|ids| ids.contains(&m.id)))
            })
            .collect();
        nodes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        nodes
    }
    fn current(&self) -> Option<&Memory> {
        self.visible().get(self.selected).copied()
    }
    fn neighbors(&self) -> Vec<(&Edge, &Memory)> {
        let Some(current) = self.current() else {
            return vec![];
        };
        self.graph
            .edges
            .iter()
            .filter_map(|edge| {
                let id = if edge.from == current.id {
                    &edge.to
                } else if edge.to == current.id {
                    &edge.from
                } else {
                    return None;
                };
                self.graph
                    .memories
                    .iter()
                    .find(|m| m.id == *id && (self.all || m.active()))
                    .map(|m| (edge, m))
            })
            .collect()
    }
    fn select_id(&mut self, id: &str) {
        self.query.clear();
        self.search_matches = None;
        self.scope = None;
        if let Some(index) = self.visible().iter().position(|m| m.id == id) {
            self.selected = index;
        }
        self.neighbor = 0;
        self.scroll = 0;
        self.focus_current();
    }
    fn refresh(&mut self, workspace: &Workspace) -> Result<()> {
        let (graph, ranks) = workspace.indexed_snapshot(&self.query)?;
        self.scene.sync(&graph);
        self.graph = graph;
        self.search_matches = (!self.query.trim().is_empty()).then(|| ranks.into_keys().collect());
        Ok(())
    }
}
#[derive(Default)]
struct MouseCapture {
    enabled: bool,
}
impl MouseCapture {
    fn set(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            execute!(io::stdout(), EnableMouseCapture, Print(pointer_sequence()))?;
        } else {
            execute!(io::stdout(), DisableMouseCapture, Print("\x1b]22;\x07"))?;
        }
        self.enabled = enabled;
        Ok(())
    }
}
// OSC 22 controls the mouse pointer, independently of the hidden text caret.
fn pointer_sequence() -> &'static str {
    let legacy = std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "iTerm.app")
        || std::env::var("LC_TERMINAL").is_ok_and(|v| v == "iTerm2")
        || std::env::var_os("XTERM_VERSION").is_some();
    if legacy {
        "\x1b]22;hand2\x07"
    } else {
        "\x1b]22;pointer\x07"
    }
}
impl Drop for MouseCapture {
    fn drop(&mut self) {
        if self.enabled {
            let _ = execute!(io::stdout(), DisableMouseCapture, Print("\x1b]22;\x07"));
        }
    }
}
pub fn run(workspace: &Workspace) -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "TUI needs an interactive terminal; use `graph --json` for agents"
    );
    let mut app = App::new(workspace.snapshot()?);
    ratatui::run(|terminal| -> io::Result<()> {
        let mut mouse = MouseCapture::default();
        mouse.set(true)?;
        let mut redraw = true;
        let mut initial_view = true;
        let mut last_frame = Instant::now();
        let mut last_paint = Instant::now() - Duration::from_millis(16);
        loop {
            let now = Instant::now();
            app.update_flow();
            redraw |= app.animate(now.duration_since(last_frame));
            last_frame = now;
            let frame_interval = Duration::from_millis(if app.motion.is_some() { 16 } else { 33 });
            if redraw && (!app.animating() || last_paint.elapsed() >= frame_interval) {
                terminal.draw(|frame| {
                    app.areas = draw(frame, &app, &workspace.project.name);
                })?;
                last_paint = Instant::now();
            }
            if initial_view && app.areas.graph.width > 1 && app.areas.graph.height > 1 {
                app.fit();
                initial_view = false;
                redraw = true;
                continue;
            }
            if app.animating() && !event::poll(frame_interval)? {
                redraw = false;
                continue;
            }
            redraw = true;
            let input = event::read()?;
            if !app.animating() {
                last_frame = Instant::now();
            }
            let key = match input {
                Event::Mouse(event) => {
                    redraw = app.mouse(event);
                    continue;
                }
                Event::Resize(_, _) | Event::FocusLost => {
                    app.drag = None;
                    app.motion = None;
                    continue;
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => key,
                _ => {
                    redraw = false;
                    continue;
                }
            };
            app.motion = None;
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                break;
            }
            if app.typing {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter => {
                        app.typing = false;
                        app.focus_current();
                    }
                    KeyCode::Backspace => {
                        app.query.pop();
                        app.selected = 0;
                    }
                    KeyCode::Char(c) => {
                        app.query.push(c);
                        app.selected = 0;
                    }
                    _ => (),
                }
                app.neighbor = 0;
                app.scroll = 0;
                app.refresh(workspace).map_err(io::Error::other)?;
                continue;
            }
            let pan = if key.modifiers.contains(KeyModifiers::SHIFT) {
                16.0
            } else {
                6.0
            };
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('/') => {
                    app.typing = true;
                    app.drag = None;
                }
                KeyCode::Char('p') => app.flow_enabled = !app.flow_enabled,
                KeyCode::Char('g') => app.graph_focus = !app.graph_focus,
                KeyCode::Char('z') => {
                    app.expanded = !app.expanded;
                    app.graph_focus = true;
                    app.drag = None;
                }
                KeyCode::Char('l') => app.scene.labels = !app.scene.labels,
                KeyCode::Char('m') => {
                    mouse.set(!mouse.enabled)?;
                    app.mouse_enabled = mouse.enabled;
                    app.drag = None;
                }
                KeyCode::Char('+') | KeyCode::Char('=') => app.zoom(1.25),
                KeyCode::Char('-') | KeyCode::Char('_') => app.zoom(0.8),
                KeyCode::Char('0') => app.fit(),
                KeyCode::Char('f') => {
                    app.focus_current();
                    app.scene.zoom = 0.85;
                }
                KeyCode::Up if app.graph_focus => app.scene.pan_cells(0.0, -pan),
                KeyCode::Down if app.graph_focus => app.scene.pan_cells(0.0, pan),
                KeyCode::Left if app.graph_focus => app.scene.pan_cells(-pan, 0.0),
                KeyCode::Right if app.graph_focus => app.scene.pan_cells(pan, 0.0),
                KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
                KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
                KeyCode::Tab => {
                    app.neighbor = (app.neighbor + 1) % app.neighbors().len().max(1);
                    app.scene.pulse();
                }
                KeyCode::Char('R') => app.scene.relayout(&app.graph),
                KeyCode::Enter | KeyCode::Right => {
                    if let Some((_, memory)) = app.neighbors().get(app.neighbor) {
                        let id = memory.id.clone();
                        if let Some(current) = app.current() {
                            app.history.push(current.id.clone());
                        }
                        app.select_id(&id);
                    }
                }
                KeyCode::Backspace | KeyCode::Left => {
                    if let Some(id) = app.history.pop() {
                        app.select_id(&id);
                    }
                }
                KeyCode::PageDown => app.scroll = app.scroll.saturating_add(8),
                KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(8),
                KeyCode::Char('a') => {
                    app.all = !app.all;
                    app.move_selection(-(app.selected as isize));
                }
                KeyCode::Char('s') => {
                    app.scope = match app.scope {
                        None => Some(Scope::Project),
                        Some(Scope::Project) => Some(Scope::Task),
                        Some(Scope::Task) => Some(Scope::Client),
                        Some(Scope::Client) => Some(Scope::Global),
                        Some(Scope::Global) => None,
                    };
                    app.move_selection(-(app.selected as isize));
                }
                KeyCode::Char('r') => {
                    app.refresh(workspace).map_err(io::Error::other)?;
                    app.selected = app.selected.min(app.visible().len().saturating_sub(1));
                    app.neighbor = 0;
                }
                _ => (),
            }
        }
        Ok(())
    })?;
    Ok(())
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(title)
}
fn draw(frame: &mut Frame, app: &App, project: &str) -> Areas {
    frame.render_widget(
        Block::default().style(Style::default().fg(Color::Gray).bg(BACKGROUND)),
        frame.area(),
    );
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .split(frame.area());
    let title = format!(
        " OMNI-GRAPH  /  {project}  /  {} memories  /  scope: {}{}",
        app.visible().len(),
        app.scope.map(|s| label(&s)).unwrap_or_else(|| "all".into()),
        if app.all { "  + history" } else { "" }
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(format!(
                " {}{}",
                if app.typing { "Search > " } else { "Search: " },
                app.query
            )),
        ]),
        rows[0],
    );
    let mut areas = Areas::default();
    if frame.area().width < 75 || frame.area().height < 16 {
        frame.render_widget(
            Paragraph::new("Use a terminal at least 75 columns × 16 rows.\nq: quit")
                .block(panel(" Terminal size ")),
            rows[1],
        );
    } else if app.expanded {
        (areas.graph, areas.labels) = draw_graph(frame, app, rows[1]);
    } else {
        let cols = Layout::horizontal([Constraint::Percentage(28), Constraint::Percentage(72)])
            .split(rows[1]);
        let visible = app.visible();
        let items: Vec<_> = visible
            .iter()
            .map(|m| {
                ListItem::new(vec![
                    Line::from(m.data.title.clone()),
                    Line::styled(
                        format!(
                            "{} · {}{}",
                            label(&m.data.kind),
                            label(&m.data.scope),
                            if m.due() { " · review due" } else { "" }
                        ),
                        Style::default().fg(MUTED),
                    ),
                ])
            })
            .collect();
        let list_block = panel(if app.graph_focus {
            " Memories · j/k select "
        } else {
            " Memories · ↑↓ / j/k select "
        })
        .border_style(Style::default().fg(if app.graph_focus {
            MUTED
        } else {
            Color::Cyan
        }));
        areas.list = list_block.inner(cols[0]);
        let list = List::new(items)
            .block(list_block)
            .highlight_style(Style::default().bg(SELECTION).fg(Color::White))
            .highlight_symbol("› ");
        let mut state =
            ListState::default().with_selected((!visible.is_empty()).then_some(app.selected));
        frame.render_stateful_widget(list, cols[0], &mut state);
        areas.list_offset = state.offset();
        let right = Layout::vertical([Constraint::Percentage(66), Constraint::Percentage(34)])
            .split(cols[1]);
        (areas.graph, areas.labels) = draw_graph(frame, app, right[0]);
        let text = if let Some(m) = app.current() {
            let d = &m.data;
            format!(
                "{}\n{}\n{} · {} · {}{}\n\n{}\n\nTriggers: {}\nContext: {}\nFailure: {}\nCause: {}\nPrevention: {}\nVerification: {}\nSources: {}\nUpdated: {}\nVerified: {}\nReview after: {}\nExpires: {}",
                d.title,
                m.id,
                label(&d.status),
                label(&d.evidence_level),
                d.progress.map(|p| label(&p)).unwrap_or_default(),
                if m.due() { " · REVIEW DUE" } else { "" },
                d.body,
                d.triggers.join("; "),
                d.context.as_deref().unwrap_or("—"),
                d.failure.as_deref().unwrap_or("—"),
                d.cause.as_deref().unwrap_or("—"),
                d.prevention.join("; "),
                d.verification.join("; "),
                d.sources.join("; "),
                m.updated_at.to_rfc3339(),
                m.verified_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "—".into()),
                d.review_after
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "—".into()),
                d.expires_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "—".into())
            )
        } else {
            "No memories match.\n\nAgents can add knowledge with:\nomni-graph remember --title \"…\" --body \"…\"\n\n/ search · s change scope · a show history".into()
        };
        let details_block = panel(" Memory · wheel / PgUp/PgDn ");
        areas.details = details_block.inner(right[1]);
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((app.scroll, 0))
                .block(details_block),
            right[1],
        );
    }
    let hints = vec![
        Line::from(if app.mouse_enabled {
            " Mouse: wheel zoom · drag background pan · drag node move · click inspect · Shift+wheel pan"
        } else {
            " Mouse disabled · m enables it · g switches graph/list keyboard focus"
        }),
        Line::from(format!(
            " {} · +/- zoom · 0 fit · f focus · z expand · l labels · Tab edge · Enter follow · / search · m mouse · q quit",
            if app.graph_focus {
                "Graph: arrows pan"
            } else {
                "List: ↑↓ select"
            }
        )),
        Line::from(
            " j/k select · Backspace back · s scope · a history · r reload · R rearrange · p pause/resume flow",
        ),
    ];
    frame.render_widget(
        Paragraph::new(hints).style(Style::default().fg(MUTED)),
        rows[2],
    );
    areas
}
fn draw_graph(frame: &mut Frame, app: &App, area: Rect) -> (Rect, Vec<(Rect, String)>) {
    let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(2)]).split(area);
    let block = panel(if app.graph_focus {
        " Graph · arrows pan / wheel zoom "
    } else {
        " Graph · click or g to focus "
    })
    .border_style(Style::default().fg(if app.graph_focus { Color::Cyan } else { MUTED }));
    let canvas_area = block.inner(chunks[0]);
    frame.render_widget(block, chunks[0]);
    let visible = app.visible();
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.graph.memories.is_empty() {
                "Your graph starts with its first memory."
            } else {
                "No memories match. Clear search or change the scope."
            }),
            canvas_area,
        );
        return (canvas_area, vec![]);
    }
    let current = app.current().map(|m| m.id.as_str());
    let neighbors = app.neighbors();
    let target = neighbors.get(app.neighbor).map(|(_, m)| m.id.as_str());
    let paint = app.scene.render(
        frame,
        canvas_area,
        &visible,
        &app.graph.edges,
        current,
        target,
    );
    let caption = if let Some((edge, memory)) = neighbors.get(app.neighbor) {
        format!(
            "{} {} {} · Enter follows",
            label(&edge.relation),
            if Some(edge.from.as_str()) == current {
                "→"
            } else {
                "←"
            },
            memory.data.title
        )
    } else {
        "Click a node to inspect it · f centers selection · 0 fits the graph".into()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(caption, Style::default().fg(Color::Yellow)),
            Line::styled(
                format!(
                    "{} nodes · {} on screen · {} links · zoom {:.0}% · {} · g switches keyboard focus",
                    visible.len(),
                    paint.on_screen,
                    paint.edges,
                    app.scene.zoom * 100.0,
                    if app.flow_enabled { "flow on · p pauses" } else { "flow paused · p resumes" }
                ),
                Style::default().fg(MUTED),
            ),
        ]),
        chunks[1],
    );
    (canvas_area, paint.labels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    fn example_graph(count: usize) -> Graph {
        let stamp = now();
        let memories: Vec<_> = (0..count)
            .map(|i| Memory {
                id: format!("memory-{i}"),
                project_id: Some("project-test".into()),
                client_id: None,
                created_at: stamp,
                updated_at: stamp,
                verified_at: None,
                data: MemoryInput {
                    title: format!("Memory {i}"),
                    ..MemoryInput::default()
                },
            })
            .collect();
        let edges = (1..count)
            .map(|i| Edge {
                from: memories[0].id.clone(),
                to: memories[i].id.clone(),
                relation: Relation::RelatedTo,
            })
            .collect();
        Graph {
            schema_version: 1,
            memories,
            edges,
        }
    }
    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }
    fn scene_app() -> App {
        let mut app = App::new(example_graph(3));
        app.areas.graph = Rect::new(30, 5, 100, 30);
        app.areas.list = Rect::new(1, 5, 27, 30);
        app.areas.details = Rect::new(30, 36, 100, 10);
        app.scene.center = Point::default();
        app.scene
            .positions
            .insert("memory-0".into(), Point::default());
        app.scene
            .positions
            .insert("memory-1".into(), Point { x: 30.0, y: 10.0 });
        app.scene
            .positions
            .insert("memory-2".into(), Point { x: -30.0, y: -10.0 });
        app
    }
    #[test]
    fn click_and_drag_move_a_node_without_mutating_memories_or_the_camera() {
        let mut app = scene_app();
        let before = serde_json::to_value(&app.graph).unwrap();
        let id = "memory-1";
        let point = app
            .scene
            .world_to_screen(app.scene.positions[id], app.areas.graph);
        let (x, y) = (point.x.round() as u16, point.y.round() as u16);
        let center = app.scene.center;
        let position = app.scene.positions[id];
        assert!(app.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y)));
        assert_eq!(app.current().unwrap().id, id);
        assert_eq!(app.scene.center, center);
        assert!(app.mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x + 5, y + 3)));
        let moved = app.scene.positions[id];
        assert!((moved.x - position.x - 5.0 / app.scene.zoom).abs() < 1e-9);
        assert!((moved.y - position.y + 6.0 / app.scene.zoom).abs() < 1e-9);
        assert_eq!(serde_json::to_value(&app.graph).unwrap(), before);
        app.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 0, 0));
        assert!(app.drag.is_none());
        assert!(!app.mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            x + 10,
            y + 6
        )));
    }
    #[test]
    fn background_drag_pans_and_wheel_zoom_stays_anchored() {
        let mut app = scene_app();
        let positions = app.scene.positions.clone();
        let center = app.scene.center;
        app.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 35, 7));
        assert!(matches!(app.drag, Some(Drag::Pan { .. })));
        app.mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 41, 10));
        assert!(app.scene.center.x < center.x && app.scene.center.y > center.y);
        assert_eq!(app.scene.positions, positions);
        app.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 41, 10));
        let cursor = Point { x: 105.0, y: 13.0 };
        let before = app.scene.screen_to_world(cursor, app.areas.graph);
        let zoom = app.scene.zoom;
        app.mouse(mouse(MouseEventKind::ScrollUp, 105, 13));
        assert_eq!(app.scene.zoom, zoom);
        app.animate(Duration::from_millis(16));
        assert!(app.scene.zoom > zoom && app.scene.zoom < zoom * 1.06);
        let after = app.scene.screen_to_world(cursor, app.areas.graph);
        assert!((after.x - before.x).abs() < 1e-9 && (after.y - before.y).abs() < 1e-9);
    }
    #[test]
    fn node_transitions_settle_and_dragged_nodes_do_not_snap_back() {
        let graph = example_graph(8);
        let original = serde_json::to_value(&graph).unwrap();
        let mut scene = Scene::new(&graph);
        let destination = scene.positions["memory-1"];
        scene.positions.insert(
            "memory-1".into(),
            Point {
                x: destination.x + 60.0,
                y: destination.y + 20.0,
            },
        );
        scene.relayout(&graph);
        let start = scene.positions["memory-1"];
        scene.animate(Duration::from_millis(33));
        let halfway = scene.positions["memory-1"];
        assert_ne!(halfway, start);
        assert_ne!(halfway, destination);
        let mut fine = Scene::new(&graph);
        fine.positions.insert("memory-1".into(), start);
        fine.relayout(&graph);
        for _ in 0..3 {
            fine.animate(Duration::from_millis(11));
        }
        assert!((fine.positions["memory-1"].x - halfway.x).abs() < 1e-9);
        scene.stop_node("memory-1");
        scene.animate(Duration::from_secs(1));
        assert_eq!(scene.positions["memory-1"], halfway);
        assert!(!scene.is_animating());
        assert!(!scene.animate(Duration::from_millis(33)));
        assert_eq!(serde_json::to_value(&graph).unwrap(), original);
        fine.animate(Duration::from_secs(2));
        assert_eq!(fine.positions["memory-1"], destination);
        assert!(!fine.is_animating());
    }
    #[test]
    fn newly_loaded_nodes_animate_without_moving_existing_memories() {
        let graph = example_graph(4);
        let mut initial = graph.clone();
        initial.memories.pop();
        initial.edges.pop();
        let mut scene = Scene::new(&initial);
        let positions = scene.positions.clone();
        scene.sync(&graph);
        let start = scene.positions["memory-3"];
        assert!(scene.is_animating());
        scene.animate(Duration::from_millis(33));
        assert_ne!(scene.positions["memory-3"], start);
        scene.animate(Duration::from_secs(2));
        assert!(!scene.is_animating());
        for (id, point) in positions {
            assert_eq!(scene.positions[&id], point);
        }
    }
    #[test]
    fn flow_tracks_search_visibility_and_pause_without_changing_memories() {
        let mut app = scene_app();
        let original = serde_json::to_value(&app.graph).unwrap();
        app.update_flow();
        for _ in 0..4 {
            app.animate(Duration::from_secs(2));
            assert!(app.animating());
        }
        app.flow_enabled = false;
        app.update_flow();
        app.animate(Duration::from_secs(2));
        assert!(!app.animating());
        app.flow_enabled = true;
        app.typing = true;
        app.update_flow();
        assert!(!app.animating());
        app.typing = false;
        app.query = "one result".into();
        app.search_matches = Some(HashSet::from(["memory-0".into()]));
        app.update_flow();
        assert!(!app.animating());
        app.query.clear();
        app.search_matches = None;
        app.update_flow();
        assert!(app.animating());
        assert_eq!(serde_json::to_value(&app.graph).unwrap(), original);
        app.graph.edges.clear();
        app.update_flow();
        assert!(!app.animating());
    }
    #[test]
    fn hub_growth_fills_more_solid_pixels_with_normal_sized_rings() {
        let graph = example_graph(30);
        let mut scene = Scene::new(&graph);
        let area = Rect::new(0, 0, 40, 20);
        for zoom in [0.06, 1.0] {
            scene.zoom = zoom;
            let mut counts = vec![];
            let mut glyphs = vec![];
            for memory in [&graph.memories[0], &graph.memories[1]] {
                scene.positions.insert(memory.id.clone(), Point::default());
                let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
                terminal
                    .draw(|frame| {
                        scene.render(frame, area, &[memory], &[], None, None);
                    })
                    .unwrap();
                glyphs.push(
                    terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .map(|c| c.symbol())
                        .collect::<String>(),
                );
                counts.push(
                    terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .filter(|cell| matches!(cell.symbol(), "█" | "▀" | "▄" | "●"))
                        .count(),
                );
            }
            if zoom < 0.65 {
                assert!(glyphs[0].contains("⬤") && glyphs[1].contains("●"));
            } else {
                assert!(counts[0] > counts[1], "zoom={zoom} filled cells={counts:?}");
            }
            assert_eq!(Scene::ring_radius(&graph.memories[0], false), 3.3);
            assert_eq!(Scene::ring_radius(&graph.memories[1], false), 3.3);
        }
    }
    #[test]
    fn connected_nodes_grow_and_hit_testing_tracks_their_visible_radius() {
        let mut graph = example_graph(30);
        let mut scene = Scene::new(&graph);
        scene.zoom = 1.0;
        let hub = &graph.memories[0];
        let leaf = &graph.memories[1];
        assert!(scene.body_radius(hub) > scene.body_radius(leaf));
        assert!(scene.body_radius(hub) <= scene.body_radius(leaf) * 2.0);
        assert_eq!(
            Scene::ring_radius(hub, false),
            Scene::ring_radius(leaf, false)
        );
        assert_eq!(Scene::ring_radius(hub, false), 3.3);
        let radius = scene.body_radius(hub);
        graph.edges.push(graph.edges[0].clone());
        scene.sync(&graph);
        assert_eq!(scene.body_radius(&graph.memories[0]), radius);
        scene.zoom = 0.06;
        let hub = &graph.memories[0];
        assert!(scene.body_radius(hub) * scene.zoom >= 0.9);
        let area = Rect::new(5, 5, 100, 30);
        let center = scene.positions[&hub.id];
        scene.center = center;
        let point = scene.world_to_screen(
            Point {
                x: center.x + scene.radius(hub, false) - 0.5,
                y: center.y,
            },
            area,
        );
        assert_eq!(
            scene.hit(std::iter::once(hub), point, area, None),
            Some(hub.id.clone())
        );
        graph.edges.clear();
        scene.sync(&graph);
        assert!(scene.radius(&graph.memories[0], false) * scene.zoom < 1.0);
    }
    #[test]
    fn wheel_bursts_accumulate_and_animation_settles_without_camera_drift() {
        let mut app = scene_app();
        let point = Point { x: 105.0, y: 13.0 };
        let anchor = app.scene.screen_to_world(point, app.areas.graph);
        let start = app.scene.zoom;
        for _ in 0..8 {
            app.mouse(mouse(MouseEventKind::ScrollUp, 105, 13));
        }
        assert_eq!(app.scene.zoom, start);
        for _ in 0..60 {
            app.animate(Duration::from_millis(16));
            let actual = app.scene.screen_to_world(point, app.areas.graph);
            assert!((actual.x - anchor.x).abs() < 1e-9 && (actual.y - anchor.y).abs() < 1e-9);
        }
        assert!(app.motion.is_none());
        assert!((app.scene.zoom - start * 1.06_f64.powi(8)).abs() < 1e-9);
        app.mouse(mouse(MouseEventKind::ScrollDown, 105, 13));
        app.fit();
        let fitted = app.scene.zoom;
        assert!(!app.animate(Duration::from_secs(1)));
        assert_eq!(app.scene.zoom, fitted);
        app.mouse(mouse(MouseEventKind::ScrollUp, 105, 13));
        app.mouse(mouse(MouseEventKind::Down(MouseButton::Right), 35, 7));
        assert!(app.motion.is_none());
    }
    #[test]
    fn background_covers_labels_panels_and_small_or_expanded_views() {
        for count in [0, 3] {
            for (width, height) in [(120, 35), (50, 10), (1, 1)] {
                for expanded in [false, true] {
                    let mut app = App::new(example_graph(count));
                    app.expanded = expanded;
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    terminal
                        .draw(|f| {
                            draw(f, &app, "Theme test");
                        })
                        .unwrap();
                    for cell in &terminal.backend().buffer().content {
                        assert!(
                            cell.bg == BACKGROUND || cell.bg == SELECTION,
                            "unexpected background: {:?}",
                            cell.bg
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn wheel_is_routed_to_the_panel_under_the_pointer() {
        let mut app = scene_app();
        let zoom = app.scene.zoom;
        app.mouse(mouse(MouseEventKind::ScrollDown, 5, 10));
        assert_eq!(app.selected, 1);
        assert_eq!(app.scene.zoom, zoom);
        let center = app.scene.center;
        app.mouse(mouse(MouseEventKind::ScrollDown, 40, 38));
        assert_eq!(app.scroll, 3);
        assert_eq!(app.scene.center, center);
        let mut shift = mouse(MouseEventKind::ScrollUp, 70, 15);
        shift.modifiers = KeyModifiers::SHIFT;
        app.mouse(shift);
        app.animate(Duration::from_millis(16));
        assert!(app.scene.center.y > center.y);
        assert_eq!(app.scene.zoom, zoom);
    }
    #[test]
    fn filtered_nodes_cannot_be_selected_and_scrolled_list_clicks_use_its_offset() {
        let mut app = scene_app();
        app.query = "matches-one".into();
        app.search_matches = Some(HashSet::from(["memory-0".into()]));
        let point = app
            .scene
            .world_to_screen(app.scene.positions["memory-1"], app.areas.graph);
        app.mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            point.x.round() as u16,
            point.y.round() as u16,
        ));
        assert_eq!(app.current().unwrap().id, "memory-0");
        assert!(matches!(app.drag, Some(Drag::Pan { .. })));
        app.query.clear();
        app.search_matches = None;
        app.areas.list_offset = 1;
        app.mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            5,
            app.areas.list.y,
        ));
        assert_eq!(app.current().unwrap().id, "memory-1");
        assert!(!app.graph_focus);
    }
    #[test]
    fn fit_shows_all_100_nodes_and_expanded_view_uses_the_available_canvas() {
        let graph = example_graph(100);
        let mut app = App::new(graph.clone());
        let positions = app.scene.positions.clone();
        let mut terminal = Terminal::new(TestBackend::new(150, 42)).unwrap();
        terminal
            .draw(|f| {
                app.areas = draw(f, &app, "Demo");
            })
            .unwrap();
        let small = app.areas.graph;
        app.fit();
        for point in app.scene.positions.values() {
            let screen = app.scene.world_to_screen(*point, app.areas.graph);
            assert!(
                contains(app.areas.graph, screen),
                "point={point:?} screen={screen:?} area={:?} center={:?} zoom={}",
                app.areas.graph,
                app.scene.center,
                app.scene.zoom
            );
        }
        app.expanded = true;
        terminal
            .draw(|f| {
                app.areas = draw(f, &app, "Demo");
            })
            .unwrap();
        assert!(app.areas.graph.width > small.width && app.areas.graph.height > small.height);
        assert_eq!(app.scene.positions, positions);
        assert_eq!(app.scene.positions.len(), 100);
        assert_eq!(
            serde_json::to_value(&app.graph).unwrap(),
            serde_json::to_value(graph).unwrap()
        );
    }
    #[test]
    fn terminal_search_uses_fts_and_following_a_node_clears_the_filter() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace {
            root: temp.path().join("project"),
            home: temp.path().join("home"),
            project: Project {
                schema_version: 1,
                id: id("project"),
                name: "Search test".into(),
                client_id: None,
                kind: "test".into(),
                description: String::new(),
                repository: None,
                technologies: vec![],
                status: "active".into(),
            },
        };
        let memory = workspace
            .create(MemoryInput {
                title: "Café deployment".into(),
                ..MemoryInput::default()
            })
            .unwrap();
        let mut app = App::new(workspace.snapshot().unwrap());
        app.query = "cafe".into();
        app.refresh(&workspace).unwrap();
        assert_eq!(app.visible().len(), 1);
        assert_eq!(app.current().unwrap().id, memory.id);
        app.query = "nonexistent".into();
        app.refresh(&workspace).unwrap();
        assert!(app.visible().is_empty());
        app.select_id(&memory.id);
        assert_eq!(app.current().unwrap().id, memory.id);
    }
    #[test]
    fn empty_and_small_terminals_render_without_panics() {
        let app = App::new(Graph::default());
        for (width, height) in [(120, 35), (50, 10), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|f| {
                    draw(f, &app, "Test project");
                })
                .unwrap();
            if width == 120 {
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                assert!(text.contains("Your graph starts"));
            }
        }
    }
}
