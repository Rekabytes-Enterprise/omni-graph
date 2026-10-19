//! Native GitHub updater. Transport is separate from verified staging/replacement.
use anyhow::{Context, Result, ensure};
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const REPO: &str = "https://github.com/Rekabytes-Enterprise/omni-graph";
const API: &str = "https://api.github.com/repos/Rekabytes-Enterprise/omni-graph/releases/latest";
const MAX_ARCHIVE: u64 = 128 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}
fn version(value: &str) -> Result<[u64; 3]> {
    let values: Vec<_> = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .collect();
    ensure!(
        values.len() == 3
            && values.iter().all(|s| !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && (s.len() == 1 || !s.starts_with('0'))),
        "invalid stable release version: {value}"
    );
    Ok([values[0].parse()?, values[1].parse()?, values[2].parse()?])
}
fn platform() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("darwin-arm64"),
        ("macos", "x86_64") => Ok("darwin-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        ("windows", "x86_64") => Ok("win32-x64"),
        _ => anyhow::bail!("no published binary for this platform; install from source"),
    }
}
pub fn run(home: &Path, json_output: bool) -> Result<()> {
    if !json_output {
        println!("Checking GitHub Releases for updates…");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let fetch = |url: &str, limit: u64| -> Result<Vec<u8>> {
        let mut response = agent
            .get(url)
            .header(
                "User-Agent",
                concat!("omni-graph/", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .with_context(|| format!("download {url}; check your connection and retry"))?;
        let mut data = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut data)?;
        ensure!(
            data.len() as u64 <= limit,
            "release download exceeded size limit"
        );
        Ok(data)
    };
    let release: Release = serde_json::from_slice(&fetch(API, 2 * 1024 * 1024)?)?;
    ensure!(
        !release.draft && !release.prerelease,
        "latest release must be stable and published"
    );
    let current = env!("CARGO_PKG_VERSION");
    let latest = release
        .tag_name
        .strip_prefix('v')
        .context("release tag must start with v")?;
    if version(latest)? <= version(current)? {
        if json_output {
            println!(
                "{}",
                json!({"updated":false,"version":current,"latest":latest})
            );
        } else {
            println!("Omni-graph {current} is up to date (latest release: {latest}).");
        }
        return Ok(());
    }
    let asset = format!("omni-graph-{}.tar.gz", platform()?);
    let base = format!("{REPO}/releases/download/{}", release.tag_name);
    if !json_output {
        println!("Downloading Omni-graph {latest}…");
    }
    let checksums = String::from_utf8(fetch(&format!("{base}/SHA256SUMS"), 64 * 1024)?)?;
    let archive = fetch(&format!("{base}/{asset}"), MAX_ARCHIVE)?;
    let binary = apply(home, latest, &asset, &checksums, &archive)?;
    if json_output {
        println!(
            "{}",
            json!({"updated":true,"previous":current,"version":latest,"binary":binary,"home":home})
        );
    } else {
        println!(
            "Updated Omni-graph {current} → {latest}\nExecutable: {}\nExisting memories and project files are preserved.",
            binary.display()
        );
    }
    Ok(())
}
fn apply(
    home: &Path,
    expected_version: &str,
    asset: &str,
    checksums: &str,
    archive: &[u8],
) -> Result<PathBuf> {
    let matches: Vec<_> = checksums
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            (name == asset).then_some(hash)
        })
        .collect();
    ensure!(
        matches.len() == 1
            && matches[0].len() == 64
            && matches[0].bytes().all(|b| b.is_ascii_hexdigit()),
        "missing or invalid release checksum; nothing was installed"
    );
    ensure!(
        format!("{:x}", Sha256::digest(archive)).eq_ignore_ascii_case(matches[0]),
        "checksum mismatch; nothing was installed"
    );
    let temp = tempfile::tempdir()?;
    let name = if cfg!(windows) {
        "omni-graph.exe"
    } else {
        "omni-graph"
    };
    let candidate = temp.path().join(name);
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    let mut found = false;
    for entry in tar.entries()? {
        let mut entry = entry?;
        ensure!(
            entry.path()?.as_ref() == Path::new(name)
                && entry.header().entry_type().is_file()
                && !found,
            "unexpected release archive entry; nothing was installed"
        );
        ensure!(
            entry.size() <= MAX_ARCHIVE,
            "release executable exceeds size limit"
        );
        let mut file = fs::File::create(&candidate)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
        file.sync_all()?;
        found = true;
    }
    ensure!(found, "release archive has no executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755))?;
    }
    let output = Command::new(&candidate)
        .arg("--version")
        .output()
        .context("validate downloaded executable")?;
    ensure!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim()
                == format!("omni-graph {expected_version}"),
        "downloaded executable version does not match release; nothing was installed"
    );
    crate::install::replace(home, &candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(payload: &[u8], entry: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, entry, payload).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }
    #[test]
    fn stable_versions_compare_numerically_and_reject_unsafe_tags() {
        assert!(version("v0.10.0").unwrap() > version("0.2.9").unwrap());
        for tag in [
            "v0.2",
            "0.2.1-beta",
            "../x",
            "00.2.1",
            "0.2.1/extra",
            "0.2.-1",
        ] {
            assert!(version(tag).is_err());
        }
    }
    #[test]
    fn corrupt_checksums_and_unexpected_entries_preserve_installation() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join("bin")).unwrap();
        let target = home.path().join("bin").join(if cfg!(windows) {
            "omni-graph.exe"
        } else {
            "omni-graph"
        });
        fs::write(&target, b"previous").unwrap();
        let data = archive(b"invalid", "unexpected");
        let valid = format!("{:x}  test.tar.gz", Sha256::digest(&data));
        for sums in [
            "".to_owned(),
            format!("{}  test.tar.gz", "0".repeat(64)),
            valid,
        ] {
            assert!(apply(home.path(), "0.2.1", "test.tar.gz", &sums, &data).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"previous");
        }
    }
    #[cfg(unix)]
    #[test]
    fn verified_update_preserves_data_and_rejects_version_mismatch() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("memory.sqlite"), b"preserved data").unwrap();
        let data = archive(b"#!/bin/sh\nprintf 'omni-graph 0.2.1\\n'\n", "omni-graph");
        let sums = format!("{:x}  test.tar.gz", Sha256::digest(&data));
        assert!(apply(home.path(), "0.2.2", "test.tar.gz", &sums, &data).is_err());
        assert!(!home.path().join("bin/omni-graph").exists());
        let target = apply(home.path(), "0.2.1", "test.tar.gz", &sums, &data).unwrap();
        assert!(
            Command::new(target)
                .arg("--version")
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            fs::read(home.path().join("memory.sqlite")).unwrap(),
            b"preserved data"
        );
    }
}
