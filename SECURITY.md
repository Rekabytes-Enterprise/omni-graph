# Security policy

## Reporting a vulnerability

Do not publish private memories, credentials, working exploits, or details that
would expose another user's data in a public issue or pull request.

Use GitHub's [Report a vulnerability](https://github.com/Rekabytes-Enterprise/omni-graph/security/advisories/new)
form when private vulnerability reporting is available. If the form is unavailable,
open a public issue titled **Private security contact requested**, containing only
a request for a private reporting channel. A maintainer will arrange a channel
before requesting sensitive details. A `SECURITY.md` file alone does not enable
GitHub's private reporting feature.

Privately include the affected version/platform, impact, a minimal reproduction
using synthetic data, and any suggested mitigation. Never send real client memory
databases, tokens, email PINs, or unrelated personal information. Please coordinate
disclosure with the maintainer while a fix is investigated. There is no guaranteed
response time or bug bounty program.

## Supported versions

Security fixes target the latest stable GitHub release. Older releases and `dev`
do not have a separate security maintenance schedule. Upgrade before checking
whether an issue persists. If an upgrade is impossible, include that constraint in
your report; backports are considered case by case.

## Security boundaries

- Memories live in local SQLite files. Treat them as sensitive project data;
  protect their filesystem permissions and backups. Local storage is not a claim
  of encryption at rest.
- The Web UI listens on loopback by default. Remote mode deliberately sends
  traffic through Cloudflare and requires allowed-email/PIN authentication.
  Keep the terminal session and permitted email accounts secure; stop the session
  to revoke its temporary link.
- Agent skills provide instructions. Recalled memory remains untrusted context
  and cannot grant permission to execute commands or override user instructions.
- Release installers verify published SHA-256 checksums. Checksums detect damaged
  or mismatched downloads; they are not an independent signature of the publisher.

For installation or usage problems without a security impact, use [support](SUPPORT.md).
