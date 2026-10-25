//! Explicit, retryable store preparation. Discovery remains read-only; each store's
//! migration is atomic, but preparation never claims a transaction across stores.
use crate::{
    cli::Cli,
    compatibility::{self, ContractError, Status, StoreReport},
    model::Scope,
    storage,
};
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Unchanged,
    Migrated,
    Initialized,
    Imported,
    Blocked,
    Failed,
    NotAttempted,
}
#[derive(Serialize)]
pub struct StorePreparation {
    pub path: PathBuf,
    pub before: StoreReport,
    pub after: StoreReport,
    pub action: Action,
    pub error: Option<ContractError>,
}
#[derive(Serialize)]
pub struct Report {
    pub integration_api_version: u32,
    pub ready: bool,
    pub exit_code: i32,
    pub stores: Vec<StorePreparation>,
    pub compatibility: compatibility::Report,
    pub skipped_registrations: Vec<crate::upgrade::SkippedRegistration>,
}
pub(crate) fn allowed(status: Status, initialize: bool) -> bool {
    matches!(status, Status::Compatible | Status::MigrationRequired)
        || (initialize
            && matches!(
                status,
                Status::MissingStore | Status::LegacyMigrationRequired
            ))
}
fn failure(error: anyhow::Error) -> ContractError {
    if let Some(error) = error.downcast_ref::<crate::database::DatabaseError>() {
        return ContractError::new(
            error.code,
            error.message.clone(),
            error.recovery,
            if matches!(error.code, "unsupported_schema" | "schema_changed") {
                12
            } else {
                1
            },
        );
    }
    ContractError::new(
        "operation_failed",
        "Store preparation failed; the store was not confirmed ready.",
        "Inspect compatibility and storage permissions. Preserve the store and any backup; retry preparation after resolving the failure.",
        1,
    )
}

pub fn prepare(paths: Vec<PathBuf>, initialize: bool) -> Result<Report> {
    let mut unique = Vec::new();
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    let before = compatibility::inspect(unique.clone())?;
    // Detect known blockers across all stores before opening any writer. This is
    // advisory: operation-time validation still handles changes after inspection.
    let blocked = before.stores.iter().any(|s| !allowed(s.status, initialize));
    let mut failed = false;
    let mut outcomes = Vec::new();
    for store in before.stores {
        let (action, error) = if store.status == Status::Compatible {
            (Action::Unchanged, None)
        } else if !allowed(store.status, initialize) {
            (Action::Blocked, None)
        } else if blocked || failed {
            (Action::NotAttempted, None)
        } else {
            match (storage::Store {
                path: store.path.clone(),
            })
            .connection()
            {
                Ok(connection) => {
                    drop(connection);
                    let action = match store.status {
                        Status::MissingStore => Action::Initialized,
                        Status::LegacyMigrationRequired => Action::Imported,
                        _ => Action::Migrated,
                    };
                    (action, None)
                }
                Err(error) => {
                    failed = true;
                    (Action::Failed, Some(failure(error)))
                }
            }
        };
        outcomes.push((store, action, error));
    }
    let compatibility = compatibility::inspect(unique)?;
    let ready = !blocked && !failed && compatibility.status == Status::Compatible;
    let exit_code = if ready {
        0
    } else {
        compatibility.exit_code.max(1)
    };
    let stores = outcomes
        .into_iter()
        .zip(&compatibility.stores)
        .map(|((before, action, error), after)| StorePreparation {
            path: before.path.clone(),
            before,
            after: after.clone(),
            action,
            error,
        })
        .collect();
    Ok(Report {
        integration_api_version: compatibility::API_VERSION,
        ready,
        exit_code,
        stores,
        compatibility,
        skipped_registrations: Vec::new(),
    })
}

pub fn run(
    cli: &Cli,
    scope: Option<Scope>,
    store: Option<&Path>,
    registered: bool,
    initialize: bool,
) -> Result<i32> {
    let start = cli.directory.clone().unwrap_or(std::env::current_dir()?);
    let home = std::path::absolute(storage::home()?)?;
    let (paths, skipped) = if registered {
        let selection = crate::upgrade::registered_paths(&home)?;
        (selection.paths, selection.skipped)
    } else {
        let paths = compatibility::selected_paths(&start, home, scope, store).map_err(|_| ContractError::new(
            "store_selection_failed", "Cannot resolve the selected memory stores.",
            "Check --directory, --scope, --store and OMNI_GRAPH_HOME. Project/client scope requires an initialized project.", 16))?;
        (paths, Vec::new())
    };
    let mut report = prepare(paths, initialize)?;
    report.skipped_registrations = skipped;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(report.exit_code)
}
