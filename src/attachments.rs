//! Scoped image bytes are separate from graph snapshots and full-text records.
use crate::{model::*, storage::Workspace};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::PathBuf};

pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGES: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageInput {
    pub file: PathBuf,
    #[serde(default)]
    pub caption: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub id: String,
    pub memory_id: String,
    pub name: String,
    pub caption: String,
    pub media_type: String,
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Debug, Clone)]
pub struct PreparedImage {
    pub metadata: Image,
    pub data: Vec<u8>,
}
fn media_type(data: &[u8]) -> Result<&'static str> {
    ensure!(
        !data.is_empty() && data.len() <= MAX_BYTES,
        "image must be between 1 byte and 8 MiB"
    );
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok("image/png");
    }
    if data.starts_with(b"\xff\xd8\xff") {
        return Ok("image/jpeg");
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Ok("image/webp");
    }
    anyhow::bail!("unsupported image: use PNG, JPEG, or WebP (SVG is not supported)")
}
impl ImageInput {
    pub fn prepare(&self, memory_id: &str) -> Result<PreparedImage> {
        let file = File::open(&self.file)
            .with_context(|| format!("read image {}", self.file.display()))?;
        ensure!(
            file.metadata()?.is_file(),
            "image input must be a regular file"
        );
        let mut data = vec![];
        file.take(MAX_BYTES as u64 + 1).read_to_end(&mut data)?;
        let media_type = media_type(&data)?.to_owned();
        ensure!(
            self.caption.chars().count() <= 2000,
            "image caption must be at most 2000 characters"
        );
        let name = self
            .file
            .file_name()
            .context("image filename is missing")?
            .to_string_lossy()
            .into_owned();
        Ok(PreparedImage {
            metadata: Image {
                id: id("img"),
                memory_id: memory_id.into(),
                name,
                caption: self.caption.clone(),
                media_type,
                bytes: data.len(),
                sha256: format!("{:x}", Sha256::digest(&data)),
            },
            data,
        })
    }
}
pub fn insert(connection: &Connection, image: &PreparedImage) -> Result<()> {
    let existing: Option<(String, Vec<u8>)> = {
        use rusqlite::OptionalExtension;
        connection
            .query_row(
                "SELECT metadata,data FROM memory_images WHERE id=?1",
                [&image.metadata.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
    };
    if let Some((metadata, data)) = existing {
        ensure!(
            serde_json::from_str::<Image>(&metadata)? == image.metadata && data == image.data,
            "conflicting image ID"
        );
        return Ok(());
    }
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM memory_images WHERE memory_id=?1",
        [&image.metadata.memory_id],
        |r| r.get(0),
    )?;
    ensure!(
        count < MAX_IMAGES as i64,
        "a memory supports at most 8 images"
    );
    connection.execute(
        "INSERT INTO memory_images(id,memory_id,metadata,data) VALUES(?1,?2,?3,?4)",
        params![
            image.metadata.id,
            image.metadata.memory_id,
            serde_json::to_string(&image.metadata)?,
            image.data
        ],
    )?;
    Ok(())
}
pub fn list(connection: &Connection, memory: &str) -> Result<Vec<Image>> {
    let mut statement = connection
        .prepare("SELECT metadata FROM memory_images WHERE memory_id=?1 ORDER BY rowid")?;
    let rows = statement.query_map([memory], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
pub fn read(workspace: &Workspace, memory: &str, image: &str) -> Result<PreparedImage> {
    ensure!(valid_id(image, "img"), "invalid image ID");
    let record = workspace.find(memory)?;
    let connection = workspace.store(record.data.scope)?.connection()?;
    let (metadata, data): (String, Vec<u8>) = connection
        .query_row(
            "SELECT metadata,data FROM memory_images WHERE memory_id=?1 AND id=?2",
            params![memory, image],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .context("image not found for this memory")?;
    let prepared = PreparedImage {
        metadata: serde_json::from_str(&metadata)?,
        data,
    };
    ensure!(
        prepared.metadata.memory_id == memory && prepared.metadata.id == image,
        "image ownership mismatch"
    );
    ensure!(
        prepared.metadata.bytes == prepared.data.len()
            && prepared.metadata.media_type == media_type(&prepared.data)?
            && prepared.metadata.sha256 == format!("{:x}", Sha256::digest(&prepared.data)),
        "image checksum mismatch"
    );
    Ok(prepared)
}

/// Portable bundles include bytes only for explicitly shared memory owners.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedImage {
    pub metadata: Image,
    pub base64: String,
}
impl SharedImage {
    pub fn from_image(image: PreparedImage) -> Self {
        Self {
            metadata: image.metadata,
            base64: STANDARD.encode(image.data),
        }
    }
    pub fn prepare(self) -> Result<PreparedImage> {
        ensure!(
            self.base64.len() <= MAX_BYTES.div_ceil(3) * 4,
            "image bundle entry is too large"
        );
        let data = STANDARD
            .decode(&self.base64)
            .context("invalid image base64")?;
        ensure!(
            valid_id(&self.metadata.id, "img") && valid_id(&self.metadata.memory_id, "mem"),
            "invalid image identity"
        );
        ensure!(
            media_type(&data)? == self.metadata.media_type
                && data.len() == self.metadata.bytes
                && format!("{:x}", Sha256::digest(&data)) == self.metadata.sha256,
            "image metadata/checksum mismatch"
        );
        ensure!(
            self.metadata.caption.chars().count() <= 2000
                && self.metadata.name.chars().count() <= 255,
            "invalid image description"
        );
        Ok(PreparedImage {
            metadata: self.metadata,
            data,
        })
    }
}
