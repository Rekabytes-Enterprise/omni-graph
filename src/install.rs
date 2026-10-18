//! Persistent user installation shared by npm and the standalone shell installer.
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
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "version": env!("CARGO_PKG_VERSION"), "home": home,
                "binary": target, "path_modified": false,
                "next": "Add the bin directory to PATH, then run omni-graph init in your project."
            }))?
        );
    } else {
        println!(
            "Installed Omni-graph {}\n\nExecutable: {}\nMemory home: {}",
            env!("CARGO_PKG_VERSION"),
            target.display(),
            home.display()
        );
        #[cfg(unix)]
        println!(
            "\nAdd this to your shell profile, then open a new terminal:\n\n  export PATH='{}':\"$PATH\"",
            bin.display().to_string().replace('\'', "'\\''")
        );
        #[cfg(windows)]
        println!(
            "\nAdd {} to your user PATH, then open a new terminal.",
            bin.display()
        );
        println!(
            "\nIn your project, run:\n\n  omni-graph init\n  omni-graph\n\nExisting memories and project files are preserved."
        );
    }
    Ok(())
}
