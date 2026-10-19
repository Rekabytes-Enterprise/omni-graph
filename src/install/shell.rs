//! User shell startup configuration; installation cannot alter its parent shell.
#[cfg(unix)]
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{fs, io::Write};

#[cfg(unix)]
const START: &str = "# >>> omni-graph PATH >>>";
#[cfg(unix)]
const END: &str = "# <<< omni-graph PATH <<<";
#[derive(Default)]
pub(super) struct Setup {
    pub profiles: Vec<PathBuf>,
    pub modified: bool,
    pub warnings: Vec<String>,
    pub command: String,
}
#[cfg(unix)]
pub(super) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(unix)]
pub(super) fn configure(bin: &Path) -> Setup {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let name = Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sh");
    let quoted = quote(&bin.to_string_lossy());
    let mut setup = Setup {
        command: format!("export PATH={quoted}:\"$PATH\""),
        ..Setup::default()
    };
    let Some(home) = std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    else {
        setup
            .warnings
            .push("HOME is unset; add the bin directory to PATH manually.".into());
        return setup;
    };
    let (files, body) = if name == "fish" {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        setup.command = format!("set -gx PATH {quoted} $PATH");
        (
            vec![config.join("fish/conf.d/omni-graph.fish")],
            format!("if not contains -- {quoted} $PATH\n    set -gx PATH {quoted} $PATH\nend"),
        )
    } else {
        let files = if name == "zsh" {
            let base = std::env::var_os("ZDOTDIR")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or(home.clone());
            vec![base.join(".zshrc"), base.join(".zprofile")]
        } else if name == "bash" {
            let login = [".bash_profile", ".bash_login", ".profile"]
                .into_iter()
                .map(|name| home.join(name))
                .find(|p| p.exists())
                .unwrap_or_else(|| home.join(".profile"));
            vec![home.join(".bashrc"), login]
        } else {
            vec![home.join(".profile")]
        };
        let pattern = quote(&format!(":{}:", bin.display()));
        (
            files,
            format!(
                "case \":$PATH:\" in\n    *{pattern}*) ;;\n    *) export PATH={quoted}:\"$PATH\" ;;\nesac"
            ),
        )
    };
    for file in files {
        match save(&file, &body) {
            Ok(changed) => {
                setup.modified |= changed;
                setup.profiles.push(file);
            }
            Err(error) => setup
                .warnings
                .push(format!("Could not configure {}: {error:#}", file.display())),
        }
    }
    setup
}

#[cfg(windows)]
pub(super) fn configure(bin: &Path) -> Setup {
    Setup {
        command: format!(
            "$env:Path = '{};' + $env:Path",
            bin.display().to_string().replace('\'', "''")
        ),
        ..Setup::default()
    }
}

// Resolve existing links so dotfile-manager symlinks and file permissions survive.
#[cfg(unix)]
fn save(profile: &Path, body: &str) -> Result<bool> {
    let target = if fs::symlink_metadata(profile).is_ok() {
        profile.canonicalize()?
    } else {
        profile.to_path_buf()
    };
    let existing = match fs::read_to_string(&target) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let block = format!("{START}\n{body}\n{END}");
    let updated = match (existing.find(START), existing.find(END)) {
        (None, None) => format!(
            "{}{}{block}\n",
            existing,
            if existing.is_empty() || existing.ends_with('\n') {
                ""
            } else {
                "\n"
            }
        ),
        (Some(start), Some(end)) => {
            ensure!(
                end >= start
                    && existing.matches(START).count() == 1
                    && existing.matches(END).count() == 1,
                "malformed managed PATH block; repair it before reinstalling"
            );
            format!(
                "{}{}{}",
                &existing[..start],
                block,
                &existing[end + END.len()..]
            )
        }
        _ => anyhow::bail!("incomplete managed PATH block; repair it before reinstalling"),
    };
    if updated == existing {
        return Ok(false);
    }
    let parent = target
        .parent()
        .context("shell profile has no parent directory")?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(meta) = fs::metadata(&target) {
        ensure!(!meta.permissions().readonly(), "shell profile is read-only");
        staged.as_file().set_permissions(meta.permissions())?;
    }
    staged.write_all(updated.as_bytes())?;
    staged.as_file().sync_all()?;
    staged
        .persist(&target)
        .with_context(|| format!("save {}", target.display()))?;
    Ok(true)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn profile_updates_preserve_user_configuration_and_dotfile_links() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("dotfiles/bashrc");
        fs::create_dir(target.parent().unwrap()).unwrap();
        fs::write(&target, "# personal configuration\nalias ll='ls -l'\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let profile = temp.path().join(".bashrc");
        symlink(&target, &profile).unwrap();
        assert!(save(&profile, "export PATH='/first/bin':\"$PATH\"").unwrap());
        assert!(!save(&profile, "export PATH='/first/bin':\"$PATH\"").unwrap());
        assert!(save(&profile, "export PATH='/second/bin':\"$PATH\"").unwrap());
        let text = fs::read_to_string(&target).unwrap();
        assert!(text.starts_with("# personal configuration\nalias ll='ls -l'\n"));
        assert_eq!(text.matches(START).count(), 1);
        assert!(text.contains("/second/bin") && !text.contains("/first/bin"));
        assert!(
            fs::symlink_metadata(profile)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
    #[test]
    fn malformed_profile_blocks_are_not_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join(".profile");
        fs::write(&file, format!("# mine\n{START}\n")).unwrap();
        let before = fs::read(&file).unwrap();
        assert!(save(&file, "anything").is_err());
        assert_eq!(fs::read(&file).unwrap(), before);
    }
}
