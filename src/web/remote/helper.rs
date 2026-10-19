//! A private, pinned cloudflared installation; never modifies system packages.
use crate::storage::Lock;
use anyhow::{Context, Result, ensure};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const VERSION: &str = "2026.10.1";
const LIMIT: u64 = 128 * 1024 * 1024;
fn asset() -> Result<(&'static str, &'static str)> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => (
            "cloudflared-darwin-arm64.tgz",
            "647615df99d49cd179ea5e13ef760b9bbfb33630f6ae62f87e33dfadda60adcc",
        ),
        ("macos", "x86_64") => (
            "cloudflared-darwin-amd64.tgz",
            "3b0dbaf378337e9d7fb6cd235a76d1d6a7dee434c2d8edf9281164fb6a3837ab",
        ),
        ("linux", "x86_64") => (
            "cloudflared-linux-amd64",
            "3bfc539f45d2620c71debb7a86be0efa737ed5ccbad9eb85c787116900e5c586",
        ),
        ("linux", "aarch64") => (
            "cloudflared-linux-arm64",
            "2c65dfabf616c40efc7545d107d7a57ad5e21377e2964b7e88af19a238074a46",
        ),
        ("windows", "x86_64") => (
            "cloudflared-windows-amd64.exe",
            "4d2490e74082c5d48dbfedaf92576fa5bd1702379ab88eb7faa0cfbe8dc891b6",
        ),
        _ => anyhow::bail!(
            "Remote access is not supported on this platform; use SSH port forwarding."
        ),
    })
}
fn verified_payload(data: &[u8], asset: &str, digest: &str) -> Result<Vec<u8>> {
    ensure!(
        format!("{:x}", Sha256::digest(data)) == digest,
        "cloudflared checksum mismatch; remote access was not started"
    );
    if !asset.ends_with(".tgz") {
        return Ok(data.to_vec());
    }
    let mut archive = tar::Archive::new(GzDecoder::new(data).take(LIMIT + 1));
    let mut payload = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(
            path == Path::new("cloudflared")
                && entry.header().entry_type().is_file()
                && payload.is_none(),
            "unexpected cloudflared archive entry"
        );
        ensure!(
            entry.size() <= LIMIT,
            "cloudflared executable exceeds size limit"
        );
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        payload = Some(bytes);
    }
    payload.context("cloudflared archive has no executable")
}
pub(super) fn supports(path: &Path) -> Result<()> {
    let version = Command::new(path)
        .arg("--version")
        .output()
        .context("check cloudflared version")?;
    ensure!(
        version.status.success()
            && String::from_utf8_lossy(&version.stdout)
                .contains(&format!("cloudflared version {VERSION}")),
        "unsupported cloudflared version; protected access requires {VERSION}"
    );
    let help = Command::new(path).args(["tunnel", "--help"]).output()?;
    ensure!(
        help.status.success() && String::from_utf8_lossy(&help.stdout).contains("--allowed-mail"),
        "cloudflared does not support email/PIN protection; remote access was not started"
    );
    Ok(())
}
pub(super) fn obtain(home: &Path, stop: &AtomicBool) -> Result<PathBuf> {
    let (name, digest) = asset()?;
    let cache = home.join("tools/cloudflared").join(VERSION);
    let _lock = Lock::at(&cache)?;
    let target = cache.join(if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    });
    let checksum = cache.join("executable.sha256");
    if let (Ok(data), Ok(expected)) = (fs::read(&target), fs::read_to_string(&checksum))
        && format!("{:x}", Sha256::digest(data)) == expected.trim()
        && supports(&target).is_ok()
    {
        return Ok(target);
    }
    println!("Preparing protected remote access (downloading cloudflared {VERSION})…");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let url =
        format!("https://github.com/cloudflare/cloudflared/releases/download/{VERSION}/{name}");
    let mut response = agent
        .get(&url)
        .header(
            "User-Agent",
            concat!("omni-graph/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .context("download cloudflared; check your connection and retry")?;
    let mut data = Vec::new();
    let mut reader = response.body_mut().as_reader().take(LIMIT + 1);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        ensure!(!stop.load(Ordering::Relaxed), "Remote access cancelled");
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..count]);
    }
    ensure!(
        data.len() as u64 <= LIMIT,
        "cloudflared download exceeds size limit"
    );
    let data = verified_payload(&data, name, digest)?;
    let temp = tempfile::tempdir_in(&cache)?;
    let staged = temp.path().join(target.file_name().unwrap());
    fs::write(&staged, &data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o700))?;
    }
    supports(&staged)?;
    let mut binary = tempfile::NamedTempFile::new_in(&cache)?;
    binary.write_all(&data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        binary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    binary.as_file().sync_all()?;
    binary
        .persist(&target)
        .context("install private cloudflared helper")?;
    let mut hash = tempfile::NamedTempFile::new_in(&cache)?;
    write!(hash, "{:x}", Sha256::digest(&data))?;
    hash.as_file().sync_all()?;
    hash.persist(checksum)?;
    Ok(target)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_rejects_invalid_checksums_and_archive_entries() {
        assert!(verified_payload(b"corrupt", "cloudflared-linux-amd64", &"0".repeat(64)).is_err());
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o700);
        header.set_cksum();
        tar.append_data(&mut header, "unexpected", &b"bad"[..])
            .unwrap();
        let data = tar.into_inner().unwrap().finish().unwrap();
        let digest = format!("{:x}", Sha256::digest(&data));
        assert!(verified_payload(&data, "helper.tgz", &digest).is_err());
        assert_eq!(
            verified_payload(
                b"binary",
                "helper",
                &format!("{:x}", Sha256::digest(b"binary"))
            )
            .unwrap(),
            b"binary"
        );
    }
}
