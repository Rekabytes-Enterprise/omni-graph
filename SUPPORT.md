# Getting help

Start with the [README](README.md) for installation, agent setup, graph controls,
updates, and remote access. Run `omni-graph --help` or
`omni-graph COMMAND --help` for command options.

Search [existing issues](https://github.com/Rekabytes-Enterprise/omni-graph/issues)
before opening a report. Use the bug report form for reproducible failures and
the feature request form for proposals. For usage questions, open an issue with
a descriptive title and explain what you tried. Support is provided by volunteers;
there is no guaranteed response time.

Include:

- `omni-graph --version`, operating system and CPU architecture.
- Installation method, terminal/browser, and the exact command involved.
- Expected and actual behavior, plus a minimal reproduction with synthetic data.
- Relevant error output with personal paths, emails, credentials, and memory content removed.

`omni-graph doctor --json` can help diagnose an initialized project. Review and
redact its output before sharing it. Never attach `.omni-graph/` directories or
real client databases. Back up your memories before trying recovery steps;
deleting a database is not a general troubleshooting step.

Report suspected vulnerabilities through [SECURITY.md](SECURITY.md).
To help improve Omni-graph, see [CONTRIBUTING.md](CONTRIBUTING.md).
