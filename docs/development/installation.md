# Shell activation after installation

The native install command owns both executable placement and shell startup configuration. Both the curl installer and the GitHub-hosted npx tarball call it, so their PATH behavior stays the same.

The executable and global memory remain in `~/.omni-graph/` by default. `OMNI_GRAPH_HOME` changes that location. Shell startup files contain only a managed PATH block; they do not hold memory data.

Two approaches were considered: changing only the POSIX curl script, or putting shell setup in the native installer. The latter covers both supported distribution paths and returns structured activation information for agents.

`src/install/shell.rs` owns profile selection, shell quoting, managed-block replacement, symlink handling and atomic writes. Its `configure(bin)` result reports configured profiles, whether files changed, warnings, and an activation command. Failed profile writes do not discard a successful binary installation. The native install output must not imply that its parent terminal has already been activated.

For Bash, update `.bashrc` and the first existing login profile in `.bash_profile`, `.bash_login`, `.profile` order; create `.profile` if none exists. For zsh, update `.zshrc` and `.zprofile` under absolute `ZDOTDIR` or HOME. Fish uses `fish/conf.d/omni-graph.fish` under absolute XDG_CONFIG_HOME or HOME/.config. Other POSIX shells use `.profile`. Windows still requires a user PATH setting and receives a PowerShell activation command.

A child process cannot change its parent's environment. Therefore every installer must clearly print the command to run in the current terminal and offer a direct executable path. New terminals discover the command through the saved startup configuration. Do not use sudo, change system profiles, or source arbitrary user profiles inside the installer.

Tests isolate HOME as well as OMNI_GRAPH_HOME so automated installations never edit a developer's shell files. Verify actual discovery in a fresh interactive shell, current-shell activation, reinstall idempotence, login-profile precedence, quoted paths, preserved dotfile links/permissions, and non-destructive error reporting.
