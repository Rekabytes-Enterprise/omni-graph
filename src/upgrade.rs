//! Registered-store selection for deliberate installation and update preparation.
//! A saved path alone is not enough: its current project identity must still match.
use crate::{
    model::{Project, valid_id},
    storage::{self, Workspace},
};
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct SkippedRegistration {
    pub path: PathBuf,
    pub reason: &'static str,
}
pub struct RegisteredSelection {
    pub paths: Vec<PathBuf>,
    pub skipped: Vec<SkippedRegistration>,
}
pub fn registered_paths(home: &Path) -> Result<RegisteredSelection> {
    let home = std::path::absolute(home)?;
    let registry = storage::registry(&home)?;
    let mut paths = vec![home.clone()];
    let mut seen = HashSet::from([home.canonicalize().unwrap_or_else(|_| home.clone())]);
    let mut skipped = vec![];
    for registration in registry.projects {
        let path = PathBuf::from(&registration.path);
        let project_file = path.join(".omni-graph/project.json");
        let reason = if !path.is_absolute() {
            Some("registered path is not absolute")
        } else {
            match storage::read::<Project>(&project_file) {
                Ok(project)
                    if project.schema_version == 1
                        && valid_id(&project.id, "project")
                        && project.id == registration.id =>
                {
                    None
                }
                Ok(_) => {
                    Some("project identity or configuration version no longer matches registration")
                }
                Err(_) => Some("registered project configuration is missing or unreadable"),
            }
        };
        if let Some(reason) = reason {
            skipped.push(SkippedRegistration { path, reason });
            continue;
        }
        let workspace = match Workspace::open(&path, home.clone()) {
            Ok(workspace) => workspace,
            Err(_) => {
                skipped.push(SkippedRegistration {
                    path,
                    reason: "registered project or client configuration is invalid",
                });
                continue;
            }
        };
        for store in workspace.stores()? {
            // Canonicalize existing directories to deduplicate project aliases.
            let identity = store
                .path
                .canonicalize()
                .unwrap_or_else(|_| store.path.clone());
            if seen.insert(identity) {
                paths.push(store.path);
            }
        }
    }
    Ok(RegisteredSelection { paths, skipped })
}
