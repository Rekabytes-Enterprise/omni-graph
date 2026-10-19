//! Protected Quick Tunnel lifetime. The provider owns PIN delivery and sign-in.
mod helper;
use anyhow::{Context, Result, ensure};
use std::{
    io::{self, BufRead, BufReader, IsTerminal, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) fn emails(values: Vec<String>) -> Result<Vec<String>> {
    let mut values = values;
    if values.is_empty() {
        ensure!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            "Remote access requires --email you@example.com in a noninteractive session"
        );
        print!("Email allowed to open this graph (Cloudflare sends a PIN at sign-in): ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        values = line.trim().split(',').map(str::to_owned).collect();
    }
    ensure!(values.len() <= 10, "allow at most 10 email addresses");
    let mut result = Vec::new();
    for value in values {
        let email = value.trim().to_ascii_lowercase();
        let valid = email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && local.len() <= 64
                && !local.starts_with('.')
                && !local.ends_with('.')
                && !local.contains("..")
                && local
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._%+-".contains(&b))
                && domain.contains('.')
                && domain.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
        });
        ensure!(
            valid && email.len() <= 254,
            "enter a full email address such as you@example.com; wildcards are not allowed"
        );
        if !result.contains(&email) {
            result.push(email);
        }
    }
    ensure!(!result.is_empty(), "at least one allowed email is required");
    Ok(result)
}
fn tunnel_url(line: &str) -> Option<String> {
    for (start, _) in line.match_indices("https://") {
        let host: String = line[start + 8..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
            .collect();
        let tail = &line[start + 8 + host.len()..];
        if tail
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace() && !matches!(c, '|' | '\'' | '"' | '/'))
        {
            continue;
        }
        if let Some(label) = host.strip_suffix(".trycloudflare.com")
            && !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Some(format!("https://{host}"));
        }
    }
    None
}
pub(super) struct Tunnel {
    pub url: String,
    child: Child,
    lines: mpsc::Receiver<String>,
    _config: tempfile::TempDir,
}
impl Tunnel {
    pub fn start(home: &Path, address: &str, emails: &[String], stop: &AtomicBool) -> Result<Self> {
        let program = helper::obtain(home, stop)?;
        ensure!(!stop.load(Ordering::Relaxed), "Remote access cancelled");
        Self::start_with(&program, address, emails, stop, Duration::from_secs(90))
    }
    fn start_with(
        program: &Path,
        address: &str,
        emails: &[String],
        stop: &AtomicBool,
        timeout: Duration,
    ) -> Result<Self> {
        ensure!(!stop.load(Ordering::Relaxed), "Remote access cancelled");
        ensure!(
            !emails.is_empty(),
            "refusing to start an unprotected tunnel"
        );
        let config = tempfile::tempdir()?;
        let path = config.path().join("empty.yaml");
        std::fs::write(&path, "{}\n")?;
        let mut command = Command::new(program);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("TUNNEL_")
                || key.to_string_lossy().starts_with("CLOUDFLARED_")
                || key == "NO_AUTOUPDATE"
            {
                command.env_remove(key);
            }
        }
        command
            .args(["tunnel", "--no-autoupdate", "--config"])
            .arg(&path)
            .args([
                "--url",
                &format!("http://{address}"),
                "--http-host-header",
                address,
                "--protocol",
                "http2",
                "--metrics",
                "127.0.0.1:0",
                "--loglevel",
                "info",
            ]);
        for email in emails {
            command.arg("--allowed-mail").arg(email);
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start protected cloudflared tunnel")?;
        let (sender, lines) = mpsc::channel();
        for stream in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let sender = sender.clone();
            thread::spawn(move || {
                for line in BufReader::new(stream)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut tunnel = Self {
            url: String::new(),
            child,
            lines,
            _config: config,
        };
        let deadline = Instant::now() + timeout;
        let mut ready = false;
        let mut last = String::new();
        while Instant::now() < deadline {
            ensure!(!stop.load(Ordering::Relaxed), "Remote access cancelled");
            if let Ok(line) = tunnel.lines.recv_timeout(Duration::from_millis(100)) {
                if let Some(url) = tunnel_url(&line) {
                    tunnel.url = url;
                }
                ready |= line.contains("Registered tunnel connection");
                if last.is_empty() || line.contains("ERR") || line.contains("Error") {
                    last = line.chars().take(500).collect();
                }
            }
            ensure!(
                tunnel.child.try_wait()?.is_none(),
                "Protected tunnel exited before connecting. {last}"
            );
            if ready && !tunnel.url.is_empty() {
                return Ok(tunnel);
            }
        }
        anyhow::bail!(
            "Timed out starting protected remote access; check outbound connectivity. {last}"
        )
    }
    pub fn check(&mut self) -> Result<()> {
        for _ in self.lines.try_iter() {}
        ensure!(
            self.child.try_wait()?.is_none(),
            "Protected tunnel exited; Web UI stopped. Restart remote access to generate a new link."
        );
        Ok(())
    }
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn email_allowlist_requires_exact_addresses() {
        assert_eq!(
            emails(vec![
                " Owner@Example.com ".into(),
                "owner@example.com".into()
            ])
            .unwrap(),
            ["owner@example.com"]
        );
        for email in [
            "",
            "*@example.com",
            "owner",
            "owner@example.com --url evil",
            "a@-bad.com",
            "a@b..com",
            "a@@b.com",
            "a\n@b.com",
        ] {
            assert!(emails(vec![email.into()]).is_err());
        }
    }
    #[test]
    fn only_provider_generated_https_hostnames_are_accepted() {
        assert_eq!(
            tunnel_url("| https://fresh-blue-river.trycloudflare.com |"),
            Some("https://fresh-blue-river.trycloudflare.com".into())
        );
        for line in [
            "http://x.trycloudflare.com",
            "https://x.trycloudflare.com.evil.com",
            "https://trycloudflare.com",
            "https://x.y.trycloudflare.com",
            "https://-x.trycloudflare.com",
            "https://good.trycloudflare.com@evil.com",
            "https://good.trycloudflare.com:8443",
        ] {
            assert!(tunnel_url(line).is_none());
        }
    }
    #[cfg(unix)]
    #[test]
    fn startup_requires_protection_and_reaps_the_helper() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("helper");
        std::fs::write(&script, "#!/bin/sh\ncase \" $* \" in *' --allowed-mail owner@example.com '*) ;; *) exit 22;; esac\nprintf 'https://fixture-only.trycloudflare.com\\nRegistered tunnel connection\\n' >&2\nwhile :; do read -r x; done\n").unwrap();
        // Use exec so the managed process itself owns the wait, without shell descendants.
        let body = std::fs::read_to_string(&script)
            .unwrap()
            .replace("while :; do read -r x; done", "exec sleep 30");
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let stop = AtomicBool::new(false);
        assert!(
            Tunnel::start_with(
                &script,
                "127.0.0.1:52716",
                &[],
                &stop,
                Duration::from_secs(1)
            )
            .is_err()
        );
        let mut tunnel = Tunnel::start_with(
            &script,
            "127.0.0.1:52716",
            &["owner@example.com".into()],
            &stop,
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(tunnel.url, "https://fixture-only.trycloudflare.com");
        let pid = tunnel.child.id();
        tunnel.check().unwrap();
        drop(tunnel);
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        stop.store(true, Ordering::Relaxed);
        assert!(
            Tunnel::start_with(
                &script,
                "127.0.0.1:52716",
                &["owner@example.com".into()],
                &stop,
                Duration::from_secs(1)
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn failed_or_timed_out_startup_stops_the_helper() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("helper");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s' \"$$\" > \"$0.pid\"\nexec sleep 30\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let stop = AtomicBool::new(false);
        let emails = ["owner@example.com".into()];
        let result = Tunnel::start_with(
            &script,
            "127.0.0.1:52716",
            &emails,
            &stop,
            Duration::from_secs(2),
        );
        assert!(result.err().unwrap().to_string().contains("Timed out"));
        let pid = std::fs::read_to_string(temp.path().join("helper.pid")).unwrap();
        assert!(
            !Command::new("kill")
                .args(["-0", &pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'ERROR authentication setup failed' >&2\nexit 23\n",
        )
        .unwrap();
        assert!(
            Tunnel::start_with(
                &script,
                "127.0.0.1:52716",
                &emails,
                &stop,
                Duration::from_secs(1)
            )
            .is_err()
        );
    }
}
