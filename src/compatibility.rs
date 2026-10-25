//! Public CLI integration contract. Discovery never opens a live store through Store.
use crate::{
    cli::Cli,
    compatibility_probe::{self, Probe},
    model::Scope,
    storage,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const API_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Compatible,
    MissingStore,
    MigrationRequired,
    UnsupportedSchema,
    PermissionDenied,
    InvalidDatabase,
    UnrecognizedDatabase,
    Busy,
    IoError,
    LegacyMigrationRequired,
}
impl Status {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Compatible => 0,
            Self::MissingStore => 10,
            Self::MigrationRequired | Self::LegacyMigrationRequired => 11,
            Self::UnsupportedSchema => 12,
            Self::PermissionDenied => 13,
            Self::InvalidDatabase | Self::UnrecognizedDatabase => 14,
            Self::Busy => 15,
            Self::IoError => 16,
        }
    }
    fn recovery(self) -> &'static str {
        match self {
            Self::Compatible => {
                "Proceed through the supported CLI. Recheck after updating the executable; operations still validate at use time."
            }
            Self::MissingStore => {
                "Initialize the intended project with init, or user memory with install, according to your installation policy."
            }
            Self::MigrationRequired => {
                "Stop older Omni-graph processes. Run prepare --json for the same selected stores to back up and migrate supported schemas, then recheck compatibility. Normal store opening also migrates."
            }
            Self::LegacyMigrationRequired => {
                "Legacy graph.json requires validation and import. Preserve the original file; use prepare --initialize --json for the same selected stores when initialization is authorized, then recheck."
            }
            Self::UnsupportedSchema => {
                "Use an executable that supports this schema. Update through the installation owner, then invoke that exact path and recheck. Do not downgrade or recreate the store."
            }
            Self::PermissionDenied => {
                "Grant the invoking user access to the selected store and retry. Do not recreate it."
            }
            Self::InvalidDatabase => {
                "Stop writers and preserve the database and WAL. Investigate or restore a matching verified backup; restoration loses writes since that backup."
            }
            Self::UnrecognizedDatabase => {
                "Select an Omni-graph store. Do not initialize over this database."
            }
            Self::Busy => {
                "Pause writes and retry inspection. The live files changed while the private snapshot was being copied."
            }
            Self::IoError => {
                "Check the selected path, storage availability and temporary-directory access, then retry."
            }
        }
    }
}
#[derive(Clone, Serialize)]
pub struct StoreReport {
    pub path: PathBuf,
    pub detected_schema_version: Option<i64>,
    pub status: Status,
    pub migration_required: bool,
    pub migration_supported: bool,
    pub recovery: &'static str,
}
#[derive(Serialize)]
pub struct Report {
    pub executable: Executable,
    pub integration_api_version: u32,
    pub supported_api_versions: Vec<u32>,
    pub database: DatabaseSupport,
    pub capabilities: Vec<&'static str>,
    pub stores: Vec<StoreReport>,
    pub status: Status,
    pub exit_code: i32,
}
#[derive(Serialize)]
pub struct Executable {
    pub version: &'static str,
    pub path: PathBuf,
}
#[derive(Serialize)]
pub struct DatabaseSupport {
    pub read_versions: Vec<i64>,
    pub write_versions: Vec<i64>,
    pub migratable_versions: Vec<i64>,
    pub migration_target: i64,
    pub migration_policy: &'static str,
}
#[derive(Debug, Serialize)]
pub struct ContractError {
    pub integration_api_version: u32,
    pub code: &'static str,
    pub message: String,
    pub recovery: &'static str,
    pub exit_code: i32,
}
impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ContractError {}
impl ContractError {
    pub fn new(
        code: &'static str,
        message: impl Into<String>,
        recovery: &'static str,
        exit_code: i32,
    ) -> Self {
        Self {
            integration_api_version: API_VERSION,
            code,
            message: message.into(),
            recovery,
            exit_code,
        }
    }
}

pub fn selected_paths(
    start: &Path,
    home: PathBuf,
    scope: Option<Scope>,
    store: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    if let Some(store) = store {
        return Ok(vec![std::path::absolute(store)?]);
    }
    if scope == Some(Scope::Global) {
        return Ok(vec![home]);
    }
    let start = start.canonicalize()?;
    let mut root = None;
    for ancestor in start.ancestors() {
        match std::fs::metadata(ancestor.join(".omni-graph/project.json")) {
            Ok(metadata) if metadata.is_file() => {
                root = Some(ancestor);
                break;
            }
            Ok(_) => anyhow::bail!("project configuration is not a file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    if let Some(root) = root {
        let workspace = storage::Workspace::open(root, home)?;
        if let Some(scope) = scope {
            return Ok(vec![workspace.store(scope)?.path]);
        }
        Ok(workspace
            .stores()?
            .into_iter()
            .map(|store| store.path)
            .collect())
    } else if scope.is_none() {
        Ok(vec![home])
    } else {
        anyhow::bail!("selected scope requires a project")
    }
}

pub fn inspect(paths: Vec<PathBuf>) -> anyhow::Result<Report> {
    let stores: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let (status, version) = match compatibility_probe::inspect(&path) {
                Probe::Missing => (Status::MissingStore, None),
                Probe::Legacy => (Status::LegacyMigrationRequired, None),
                Probe::Schema(v) if v == crate::database::VERSION => (Status::Compatible, Some(v)),
                Probe::Schema(v) if crate::database::MIGRATABLE.contains(&v) => {
                    (Status::MigrationRequired, Some(v))
                }
                Probe::Schema(v) => (Status::UnsupportedSchema, Some(v)),
                Probe::PermissionDenied => (Status::PermissionDenied, None),
                Probe::InvalidDatabase => (Status::InvalidDatabase, None),
                Probe::UnrecognizedDatabase => (Status::UnrecognizedDatabase, None),
                Probe::Busy => (Status::Busy, None),
                Probe::IoError => (Status::IoError, None),
            };
            StoreReport {
                path,
                detected_schema_version: version,
                status,
                migration_required: matches!(
                    status,
                    Status::MigrationRequired | Status::LegacyMigrationRequired
                ),
                migration_supported: status == Status::MigrationRequired,
                recovery: status.recovery(),
            }
        })
        .collect();
    // Highest exit code wins; consumers should inspect every store, not rely on ordering.
    let status = stores
        .iter()
        .map(|s| s.status)
        .max_by_key(|s| s.exit_code())
        .unwrap_or(Status::Compatible);
    Ok(Report {
        executable: Executable {
            version: env!("CARGO_PKG_VERSION"),
            path: std::env::current_exe()?.canonicalize()?,
        },
        integration_api_version: API_VERSION,
        supported_api_versions: vec![API_VERSION],
        database: DatabaseSupport {
            read_versions: vec![crate::database::VERSION],
            write_versions: vec![crate::database::VERSION],
            migratable_versions: crate::database::MIGRATABLE.to_vec(),
            migration_target: crate::database::VERSION,
            migration_policy: "automatic_on_open_with_backup",
        },
        capabilities: vec![
            "compatibility",
            "storage_preparation",
            "structured_errors",
            "memory_crud",
            "fts5_search",
            "graph_recall",
            "atomic_memory_links",
            "idempotent_creation",
            "image_attachments",
            "shared_export_import",
            "migration_backup",
            "writer_fencing",
        ],
        stores,
        status,
        exit_code: status.exit_code(),
    })
}

/// Handled before app::run so expected incompatibility never becomes a legacy error.
pub fn run(cli: &Cli, scope: Option<Scope>, store: Option<&Path>) -> anyhow::Result<i32> {
    let start = cli.directory.clone().unwrap_or(std::env::current_dir()?);
    let home = std::path::absolute(storage::home()?)?;
    let paths = selected_paths(&start, home, scope, store).map_err(|_| ContractError::new(
        "store_selection_failed", "Cannot resolve the selected memory stores.",
        "Check --directory, --scope, --store and OMNI_GRAPH_HOME. Project/client scope requires an initialized project.", 16))?;
    let report = inspect(paths)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(report.exit_code)
}
