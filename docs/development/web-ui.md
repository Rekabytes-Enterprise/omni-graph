# Interface selection and browser graph

## Problem and grounding

The no-argument command previously selected `Command::Tui`. Both interfaces must use `Workspace::snapshot`, which combines only the project's accessible global, client, project and task memories. The browser must not introduce a second storage reader or persist graph layout changes.

## Usage and shape

`omni-graph` presents Web UI / TUI, using arrows and Enter (or 1 / 2). `omni-graph tui` bypasses selection. `omni-graph web --no-open` serves the browser graph on `http://127.0.0.1:52716`; `--port` permits an explicit override. Ctrl+C stops the server.

`ui::choose() -> Result<Option<Command>>` owns terminal selection and cleanup. `web::run(&Workspace, port: u16, open: bool) -> Result<()>` hides HTTP routing, browser launch and asset delivery. The API serves a fresh scoped snapshot and server-computed active IDs. Embedded HTML, CSS and JavaScript work offline without Node or a frontend build. Browser filters, 3D layout, selection, camera and animation are session state. `GraphView` owns the Three.js scene, perspective camera, OrbitControls, ray picking and drag plane; UI code owns filters and details. Vendored Three.js 0.186.1 and OrbitControls are embedded with their MIT license; refresh them with `npm ci && npm run web:vendor`. No CDN or runtime npm installation is needed. `types.js` is the single source for memory-type colors and icons; scope remains a separate text/filter dimension. Labels are projected from the 3D scene and decluttered in priority order. HTTP routes are read-only, bound to loopback, validate Host, and deny cross-origin API requests. Memory strings render as text, never HTML.

## Candidates and synthesis

Candidate A: separate frontend dev server and JSON backend, with a browser build step and assets located at runtime. This hides frontend bundling but makes native installation coordinate two runtimes, asset discovery and server lifecycle.

Candidate B: embedded static browser app and a small HTTP server owned by the native executable. This hides asset location, deployment and snapshot policy behind one entry point. Choose B: it preserves the existing one-binary installation and storage boundary. User steering requires a true 3D graph with full azimuth orbit and tilt, so replace the initial SVG camera with an embedded Three.js view rather than simulating depth with CSS. A full async framework adds little for four static routes and one read endpoint; use tiny_http rather than hand-parsing HTTP.

## Implementation and verification

Implement the chooser, server and embedded app against these signatures. Verify noninteractive use, cancellation/terminal restoration, actual HTTP routing, workspace isolation, live refresh, missing-port errors and browser orbit/tilt/pan/zoom/selection. Keep malformed or cross-origin requests away from the snapshot handler. Accept a read-only first browser UI: agents continue writing through the existing CLI and both interfaces see those updates.
