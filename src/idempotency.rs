//! Durable creation receipts share the owning memory transaction. No TTL: an
//! acknowledged key must not silently become a new creation after time passes.
use crate::{attachments::PreparedImage, compatibility::ContractError, model::*};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

pub struct Receipt {
    namespace: String,
    key: String,
    request_hash: String,
}
impl Receipt {
    pub fn new(
        key: &str,
        memory: &Memory,
        links: &[CreateLink],
        images: &[PreparedImage],
    ) -> Result<Self> {
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err(ContractError::new("invalid_arguments", "Invalid idempotency key.",
                "Use 1–128 ASCII letters, digits, periods, underscores, colons or hyphens. Retain one key per intended creation.", 2).into());
        }
        // IDs, timestamps and absolute source paths are not request identity.
        // Image content and user-visible metadata are, so a changed file conflicts.
        let images: Vec<_> = images.iter().map(|image| {
            let m = &image.metadata;
            serde_json::json!({"name":m.name,"caption":m.caption,"media_type":m.media_type,"bytes":m.bytes,"sha256":m.sha256})
        }).collect();
        let request = serde_json::json!({"data":memory.data,"links":links,"images":images});
        let mut hash = Sha256::new();
        hash.update(b"omni-graph.creation.v1\0");
        hash.update(serde_json::to_vec(&request)?);
        Ok(Self {
            namespace: serde_json::to_string(&(
                memory.data.scope,
                &memory.project_id,
                &memory.client_id,
                &memory.data.task_id,
            ))?,
            key: key.into(),
            request_hash: format!("v1:{:x}", hash.finalize()),
        })
    }
    pub fn replay(&self, connection: &Connection) -> Result<Option<Memory>> {
        let row: Option<(String, String)> = connection
            .query_row(
                "SELECT request_hash,response FROM creation_receipts WHERE namespace=?1 AND key=?2",
                params![self.namespace, self.key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((hash, response)) = row else {
            return Ok(None);
        };
        if hash != self.request_hash {
            return Err(ContractError::new("idempotency_conflict", "This idempotency key already belongs to a different creation request.",
                "Retry the original payload with its original key, or use a new key for an intentionally different memory. No changes were made.",18).into());
        }
        Ok(Some(serde_json::from_str(&response)?))
    }
    pub fn save(&self, connection: &Connection, memory: &Memory) -> Result<()> {
        connection.execute("INSERT INTO creation_receipts(namespace,key,request_hash,response) VALUES(?1,?2,?3,?4)",
            params![self.namespace,self.key,self.request_hash,serde_json::to_string(memory)?])?;
        Ok(())
    }
}
