# Protected remote access

## Grounding and usage

The Web UI currently reads scoped snapshots through Workspace and binds loopback. The user runs on headless Ubuntu through Termius and wants a generated link usable in a browser on another device. Public exposure must require email/PIN authentication. Existing local access and SSH forwarding remain unchanged.

`omni-graph web --remote --email you@example.com` starts protected remote access; the startup selector also offers Remote and prompts for an email. No SMTP account, PIN handling or email delivery is implemented locally: Cloudflare authenticates at its edge. The user enters the PIN in their own browser.

## Candidates and synthesis

A: implement an application login, SMTP transport and tunnel wrapper. This hides provider authentication but requires SMTP credentials, PIN persistence, abuse protection and session ownership inside a memory explorer.

B: use Cloudflare's protected Quick Tunnel mode with a pinned verified helper. Choose B: remote owns one subprocess lifetime and email allowlist; Cloudflare owns delivery and authentication. The documented `--allowed-mail` flag was confirmed against the actual 2026.10.1 helper. Older installed helpers do not support it, so a private checksum-verified cache avoids silently launching an unprotected tunnel.

## Shape and invariants

`remote::emails` validates or interactively collects exact allowed addresses. `remote::Tunnel::start(home,address,emails,stop)` obtains a pinned helper, owns its isolated empty configuration, starts a tunnel with mandatory email protection, waits for its generated hostname and connection readiness, and kills/reaps it on drop. No user Cloudflare credentials/configuration are loaded. Nonzero exit, timeout, cancellation and missing authentication capability fail closed.

The web server accepts only its local authorities and the exact generated HTTPS origin; forwarded headers cannot choose an origin. The listener remains loopback. No memory response is served while tunnel setup is pending. Requests still pass method/host/origin checks and Workspace scope boundaries. Ctrl+C/SIGTERM/SIGHUP stop the server and its helper; unexpected helper exit also stops serving. SIGKILL cannot run cleanup, but a killed web process leaves no memory backend available.

Test CLI validation, hostname/email parsing, helper checksums, host/origin rejection, startup timeout/exit/cancellation and process cleanup. Live verification uses a disposable synthetic workspace and checks that anonymous HTTPS requests reach a sign-in gate rather than the memory API. The user completes real email/PIN sign-in in their own browser. Quick Tunnels are temporary sessions with no uptime guarantee, not a permanent hosting service.
