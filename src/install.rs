mod shell;

// Persistent user installation shared by npm and the standalone shell installer.
use crate::storage::{self, Lock};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{fs, path::Path};

pub fn run(home: &Path, json_output: bool) -> Result<()> {
    let source = std::env::current_exe().context("locate the running executable")?;
    // Validate existing user data before installing, and never reset it on upgrades.
    {
        let _lock = Lock::at(home)?;
        let registry = storage::registry(home)?;
        if !home.join("registry.json").exists() {
            storage::write(&home.join("registry.json"), &registry)?;
        }
    }
    let selection = crate::upgrade::registered_paths(home)?;
    let inspection = crate::compatibility::inspect(selection.paths.clone())?;
    if inspection
        .stores
        .iter()
        .any(|s| !crate::preparation::allowed(s.status, true))
    {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "installed": false, "ready": false, "compatibility": inspection,
                    "skipped_registrations": selection.skipped
                }))?
            );
            std::process::exit(inspection.exit_code.max(1));
        }
        anyhow::bail!(
            "Memory storage preflight is blocked; installation was not replaced. Run `omni-graph prepare --registered --initialize --json` to inspect per-store results. Existing memories are preserved."
        );
    }
    // Make the compatible engine durable before any migration. A later store may
    // fail after an earlier store commits; its recovery engine must remain installed.
    let target = replace(home, &source)?;
    let mut preparation = crate::preparation::prepare(selection.paths, true)?;
    preparation.skipped_registrations = selection.skipped;
    if !preparation.ready {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "installed": true, "binary": target, "ready": false,
                    "version": env!("CARGO_PKG_VERSION"), "preparation": preparation
                }))?
            );
            std::process::exit(preparation.exit_code.max(1));
        }
        anyhow::bail!(
            "The new executable is installed at {}, but memory storage preparation is incomplete. Run that executable with `prepare --registered --initialize --json` to resume. Completed migrations and backups are preserved.",
            target.display()
        );
    }
    let bin = home.join("bin");
    let path = shell::configure(&bin);
    let path_active = std::env::var_os("PATH")
        .is_some_and(|value| std::env::split_paths(&value).any(|entry| entry == bin));
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "version": env!("CARGO_PKG_VERSION"), "home": home,
                "binary": target, "ready": true, "preparation": preparation,
                "path_modified": path.modified, "path_active": path_active,
                "path_profiles": path.profiles, "warnings": path.warnings,
                "activate": path.command,
                "next": if path_active { "Run omni-graph in an initialized project. Use omni-graph init once for a new project." } else { "Run the activation command in this terminal, or open a new terminal. Use omni-graph init once for a new project, then run omni-graph." }
            }))?
        );
    } else {
        println!(
            "Installed Omni-graph {}\n\nExecutable: {}\nMemory home: {}",
            env!("CARGO_PKG_VERSION"),
            target.display(),
            home.display()
        );
        for skipped in &preparation.skipped_registrations {
            eprintln!(
                "Skipped registered project {}: {}. Reconnect it explicitly when available.",
                skipped.path.display(),
                skipped.reason
            );
        }
        if !path.profiles.is_empty() {
            println!("\nPATH saved for future terminals in:");
            for profile in &path.profiles {
                println!("  {}", profile.display());
            }
        }
        for warning in &path.warnings {
            eprintln!("Warning: {warning}");
        }
        #[cfg(windows)]
        println!(
            "\nAdd {} to your user PATH for future terminals.",
            bin.display()
        );
        if path_active {
            println!("\nReady to use in this terminal.");
        } else {
            println!(
                "\nIMPORTANT: Run this FIRST in your current terminal:\n\n  {}\n\nThe installer cannot change the PATH of the terminal that launched it.",
                path.command
            );
        }
        println!(
            "\nIn an initialized project, run:\n\n  omni-graph\n\nFor a new project, run omni-graph init once first.\nTo update the application next time, run omni-graph update from any directory."
        );
        #[cfg(unix)]
        println!(
            "\nOr use the installed executable directly without changing PATH:\n\n  {}",
            shell::quote(&target.to_string_lossy())
        );
        println!("\nExisting memories and project files are preserved.");
    }
    Ok(())
}

// Shared binary-only replacement. The caller owns subsequent readiness preparation.
pub(super) fn replace(home: &Path, source: &Path) -> Result<std::path::PathBuf> {
    let bin = home.join("bin");
    let target = bin.join(if cfg!(windows) {
        "omni-graph.exe"
    } else {
        "omni-graph"
    });
    fs::create_dir_all(&bin)?;
    let _lock = Lock::at(&bin)?;
    ensure!(
        !fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()),
        "installation destination is a symlink: {}",
        target.display()
    );
    if target.canonicalize().ok().as_ref() == Some(&source.canonicalize()?) {
        return Ok(target);
    }
    if target.canonicalize().ok().as_ref() == Some(&std::env::current_exe()?.canonicalize()?) {
        self_replace::self_replace(source).context("replace running Omni-graph executable")?;
    } else {
        let mut staged = tempfile::NamedTempFile::new_in(&bin)?;
        std::io::copy(&mut fs::File::open(source)?, staged.as_file_mut())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        staged.as_file().sync_all()?;
        staged.persist(&target).with_context(|| {
            format!(
                "install {}; close running Omni-graph instances and retry",
                target.display()
            )
        })?;
    }
    Ok(target)
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    #[test]
    fn running_installation_can_replace_itself() {
        const HOME_KEY: &str = "OMNI_GRAPH_TEST_REPLACE_HOME";
        const SOURCE_KEY: &str = "OMNI_GRAPH_TEST_REPLACE_SOURCE";
        if let Some(home) = std::env::var_os(HOME_KEY) {
            replace(
                Path::new(&home),
                Path::new(&std::env::var_os(SOURCE_KEY).unwrap()),
            )
            .unwrap();
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("memory");
        fs::create_dir_all(home.join("bin")).unwrap();
        let name = if cfg!(windows) {
            "omni-graph.exe"
        } else {
            "omni-graph"
        };
        let running = home.join("bin").join(name);
        let candidate = temp.path().join(name);
        let source = std::env::current_exe().unwrap();
        fs::copy(&source, &running).unwrap();
        fs::copy(&source, &candidate).unwrap();
        fs::write(home.join("memory.sqlite"), "keep existing memory").unwrap();
        let output = std::process::Command::new(&running)
            .args([
                "--exact",
                "install::replacement_tests::running_installation_can_replace_itself",
            ])
            .env(HOME_KEY, &home)
            .env(SOURCE_KEY, &candidate)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            std::process::Command::new(&running)
                .arg("--list")
                .output()
                .unwrap()
                .status
                .success()
        );
        assert_eq!(
            fs::read_to_string(home.join("memory.sqlite")).unwrap(),
            "keep existing memory"
        );
    }
}
