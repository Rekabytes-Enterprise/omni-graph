mod shell;

// Persistent user installation shared by npm and the standalone shell installer.
use crate::storage::{self, Lock, Store};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{fs, path::Path};

pub fn run(home: &Path, json_output: bool) -> Result<()> {
    let source = std::env::current_exe().context("locate the running executable")?;
    let bin = home.join("bin");
    let target = bin.join(if cfg!(windows) {
        "omni-graph.exe"
    } else {
        "omni-graph"
    });
    ensure!(
        !fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()),
        "installation destination is a symlink: {}",
        target.display()
    );
    // Validate existing user data before installing, and never reset it on upgrades.
    {
        let _lock = Lock::at(home)?;
        let registry = storage::registry(home)?;
        if !home.join("registry.json").exists() {
            storage::write(&home.join("registry.json"), &registry)?;
        }
    }
    Store {
        path: home.to_path_buf(),
    }
    .load()?;
    fs::create_dir_all(&bin)?;
    let _lock = Lock::at(&bin)?;
    if target.canonicalize().ok().as_ref() != Some(&source.canonicalize()?) {
        let mut staged = tempfile::NamedTempFile::new_in(&bin)?;
        std::io::copy(&mut fs::File::open(&source)?, staged.as_file_mut())?;
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
                "install {}; close any running Omni-graph instance and retry",
                target.display()
            )
        })?;
    }
    let path = shell::configure(&bin);
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "version": env!("CARGO_PKG_VERSION"), "home": home,
                "binary": target, "path_modified": path.modified,
                "path_profiles": path.profiles, "warnings": path.warnings,
                "activate": path.command,
                "next": "Run the activation command in this terminal, or open a new terminal. Then run omni-graph init from your project directory."
            }))?
        );
    } else {
        println!(
            "Installed Omni-graph {}\n\nExecutable: {}\nMemory home: {}",
            env!("CARGO_PKG_VERSION"),
            target.display(),
            home.display()
        );
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
        println!(
            "\nIMPORTANT: Run this FIRST in your current terminal:\n\n  {}\n\nThe installer cannot change the PATH of the terminal that launched it.",
            path.command
        );
        println!(
            "\nThen change to your project's directory and run:\n\n  omni-graph init\n  omni-graph"
        );
        #[cfg(unix)]
        println!(
            "\nOr use the installed executable directly without changing PATH:\n\n  {} init",
            shell::quote(&target.to_string_lossy())
        );
        println!("\nExisting memories and project files are preserved.");
    }
    Ok(())
}
