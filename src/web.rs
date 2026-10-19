//! Embedded, read-only browser graph. All memory access stays in Workspace.
use crate::storage::Workspace;
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    net::{Ipv4Addr, SocketAddr},
    process::{Command, Stdio},
};
use tiny_http::{Header, Method, Request, Response, Server};

pub fn run(workspace: &Workspace, port: u16, open: bool) -> Result<()> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let server = Server::http(address).map_err(|error| anyhow::anyhow!("cannot start Web UI on {address}: {error}. Choose another port with `omni-graph web --port PORT`."))?;
    let address = server
        .server_addr()
        .to_ip()
        .context("missing HTTP address")?;
    let url = format!("http://{address}");
    println!(
        "Omni-graph Web UI: {url}\nProject: {}\nPress Ctrl+C to stop the server.",
        workspace.project.name
    );
    if open && let Err(error) = open_browser(&url) {
        eprintln!("Could not open your browser: {error}. Open {url} manually.");
    }
    for request in server.incoming_requests() {
        if let Err(error) = respond(request, workspace, &address.to_string()) {
            eprintln!("Web request failed: {error:#}");
        }
    }
    Ok(())
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "browser launcher exited with {status}"
        )))
    }
}

fn respond(request: Request, workspace: &Workspace, address: &str) -> Result<()> {
    let header = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str())
    };
    let localhost = format!(
        "localhost:{}",
        address.rsplit(':').next().unwrap_or("52716")
    );
    let host_ok = header("Host").is_some_and(|h| h == address || h == localhost);
    let origin_ok = header("Origin")
        .is_none_or(|o| o == format!("http://{address}") || o == format!("http://{localhost}"));
    let same_site = header("Sec-Fetch-Site").is_none_or(|s| s != "cross-site");
    let (status, kind, body) = if !host_ok || !origin_ok || !same_site {
        (403, "text/plain", "Forbidden origin".to_owned())
    } else if request.method() != &Method::Get && request.method() != &Method::Head {
        (405, "text/plain", "Read-only server: use GET".to_owned())
    } else {
        match request.url().split('?').next().unwrap_or("") {
            "/" => (
                200,
                "text/html; charset=utf-8",
                include_str!("web/index.html").to_owned(),
            ),
            "/app.css" => (
                200,
                "text/css; charset=utf-8",
                include_str!("web/app.css").to_owned(),
            ),
            "/graph.js" => (
                200,
                "text/javascript; charset=utf-8",
                include_str!("web/graph.js").to_owned(),
            ),
            "/app.js" => (
                200,
                "text/javascript; charset=utf-8",
                include_str!("web/app.js").to_owned(),
            ),
            "/types.js" => (
                200,
                "text/javascript; charset=utf-8",
                include_str!("web/types.js").to_owned(),
            ),
            "/view.js" => (
                200,
                "text/javascript; charset=utf-8",
                include_str!("web/view.js").to_owned(),
            ),
            "/vendor/three.module.js" => (
                200,
                "text/javascript; charset=utf-8",
                include_str!("web/vendor/three.module.js").to_owned(),
            ),
            "/vendor/three.core.js" => (
                200,
                "text/javascript; charset=utf-8",
                include_str!("web/vendor/three.core.js").to_owned(),
            ),
            "/vendor/OrbitControls.js" => (
                200,
                "text/javascript; charset=utf-8",
                include_str!("web/vendor/OrbitControls.js").to_owned(),
            ),
            "/vendor/THREE-LICENSE.txt" => (
                200,
                "text/plain; charset=utf-8",
                include_str!("web/vendor/THREE-LICENSE.txt").to_owned(),
            ),
            "/api/graph" => match workspace.snapshot() {
                Ok(graph) => {
                    let active: Vec<_> = graph
                        .memories
                        .iter()
                        .filter(|m| m.active())
                        .map(|m| &m.id)
                        .collect();
                    (
                        200,
                        "application/json",
                        serde_json::to_string(
                            &json!({"project": workspace.project, "graph": graph, "active": active}),
                        )?,
                    )
                }
                Err(error) => {
                    eprintln!("Cannot read graph: {error:#}");
                    (
                        500,
                        "application/json",
                        json!({"error":"Cannot read memory. Check the terminal for details."})
                            .to_string(),
                    )
                }
            },
            _ => (404, "text/plain", "Not found".to_owned()),
        }
    };
    let mut response = Response::from_string(body).with_status_code(status);
    for (name, value) in [
        ("Content-Type", kind),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        (
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    ] {
        response.add_header(
            Header::from_bytes(name, value)
                .map_err(|_| anyhow::anyhow!("invalid response header"))?,
        );
    }
    request.respond(response)?;
    Ok(())
}
