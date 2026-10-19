//! Embedded, read-only browser graph. All memory access stays in Workspace.
mod remote;

use crate::{cli::Web, storage::Workspace};
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    net::{Ipv4Addr, SocketAddr},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tiny_http::{Header, Method, Request, Response, Server};

pub fn run(workspace: &Workspace, args: Web) -> Result<()> {
    // Collect the allowlist before binding or starting any network tunnel.
    let emails = if args.remote {
        remote::emails(args.email)?
    } else {
        vec![]
    };
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
        .context("install Web UI shutdown handler")?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, args.port));
    let server = Server::http(address).map_err(|error| anyhow::anyhow!("cannot start Web UI on {address}: {error}. Choose another port with `omni-graph web --port PORT`."))?;
    let address = server
        .server_addr()
        .to_ip()
        .context("missing HTTP address")?;
    let local = format!("http://{address}");
    let mut tunnel = if args.remote {
        println!("Starting email/PIN-protected remote access…");
        match remote::Tunnel::start(&workspace.home, &address.to_string(), &emails, &stop) {
            Ok(tunnel) => Some(tunnel),
            Err(_) if stop.load(Ordering::Relaxed) => return Ok(()),
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let public = tunnel.as_ref().map(|t| t.url.as_str());
    let url = public.unwrap_or(&local);
    println!(
        "Omni-graph Web UI: {url}\nProject: {}\nPress Ctrl+C to stop the server{}.",
        workspace.project.name,
        if args.remote {
            " and revoke the tunnel"
        } else {
            ""
        }
    );
    if args.remote {
        println!(
            "Email/PIN sign-in required. Allowed email(s): {}\nOpen the HTTPS link in your own browser. The link lasts only for this session.",
            emails.join(", ")
        );
    } else if !args.no_open
        && let Err(error) = open_browser(url)
    {
        eprintln!("Could not open your browser: {error}. Open {url} manually.");
    }
    // Own the hostname; no forwarded request header can change the origin policy.
    let public = public.map(str::to_owned);
    while !stop.load(Ordering::Relaxed) {
        if let Some(tunnel) = tunnel.as_mut() {
            tunnel.check()?;
        }
        if let Some(request) = server.recv_timeout(Duration::from_millis(200))?
            && let Err(error) = respond(request, workspace, &address.to_string(), public.as_deref())
        {
            eprintln!("Web request failed: {error:#}");
        }
    }
    drop(tunnel);
    println!(
        "Web UI stopped{}.",
        if args.remote { "; tunnel revoked" } else { "" }
    );
    Ok(())
}

fn allowed_authority(
    host: Option<&str>,
    origin: Option<&str>,
    site: Option<&str>,
    address: &str,
    public: Option<&str>,
    navigation: bool,
) -> bool {
    let localhost = format!(
        "localhost:{}",
        address.rsplit(':').next().unwrap_or("52716")
    );
    let public_host = public.and_then(|url| url.strip_prefix("https://"));
    let host_ok = host.is_some_and(|h| h == address || h == localhost || Some(h) == public_host);
    let origin_ok = origin.is_none_or(|o| {
        o == format!("http://{address}") || o == format!("http://{localhost}") || Some(o) == public
    });
    host_ok && origin_ok && (navigation || site.is_none_or(|s| s != "cross-site"))
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

fn respond(
    request: Request,
    workspace: &Workspace,
    address: &str,
    public: Option<&str>,
) -> Result<()> {
    let header = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str())
    };
    // Cloudflare's email sign-in redirects back as a cross-site document navigation.
    // Permit that landing page only; cross-site API and module requests stay blocked.
    let navigation = public.is_some()
        && request.url().split('?').next() == Some("/")
        && matches!(request.method(), Method::Get | Method::Head)
        && header("Sec-Fetch-Mode") == Some("navigate")
        && header("Sec-Fetch-Dest") == Some("document");
    let allowed = allowed_authority(
        header("Host"),
        header("Origin"),
        header("Sec-Fetch-Site"),
        address,
        public,
        navigation,
    );
    let (status, kind, body) = if !allowed {
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
                            &json!({"project": workspace.project, "graph": graph, "active": active, "access": if public.is_some() { "remote" } else { "local" }}),
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

#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn remote_origin_is_exact_and_does_not_trust_forwarded_headers() {
        let addr = "127.0.0.1:52716";
        let public = "https://safe-only.trycloudflare.com";
        assert!(allowed_authority(
            Some(addr),
            Some(public),
            Some("same-origin"),
            addr,
            Some(public),
            false
        ));
        assert!(allowed_authority(
            Some("safe-only.trycloudflare.com"),
            Some(public),
            None,
            addr,
            Some(public),
            false
        ));
        assert!(!allowed_authority(
            Some(addr),
            Some(public),
            None,
            addr,
            None,
            false
        ));
        for origin in [
            "https://other.trycloudflare.com",
            "https://safe-only.trycloudflare.com.evil.com",
            "http://safe-only.trycloudflare.com",
        ] {
            assert!(!allowed_authority(
                Some(addr),
                Some(origin),
                None,
                addr,
                Some(public),
                false
            ));
        }
        assert!(!allowed_authority(
            Some("evil.com"),
            Some(public),
            None,
            addr,
            Some(public),
            false
        ));
        assert!(allowed_authority(
            Some(addr),
            None,
            Some("cross-site"),
            addr,
            Some(public),
            true
        ));
        assert!(!allowed_authority(
            Some(addr),
            Some(public),
            Some("cross-site"),
            addr,
            Some(public),
            false
        ));
    }
}
