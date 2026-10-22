//! SQLite persistence. JSON is retained per row for lossless interchange; generated
//! columns, ordinary indexes, FTS5 and edges are maintained transactionally.
use crate::{model::*, storage};
use anyhow::{Context, Result, ensure};
use rusqlite::{
    Connection, OpenFlags, Transaction, TransactionBehavior, functions::FunctionFlags, params,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::Duration,
};

/// Stable database failures exposed by the public integration envelope.
#[derive(Debug)]
pub struct DatabaseError {
    pub code: &'static str,
    pub recovery: &'static str,
    pub message: String,
}
impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for DatabaseError {}
fn failure(code: &'static str, recovery: &'static str, message: String) -> anyhow::Error {
    DatabaseError {
        code,
        recovery,
        message,
    }
    .into()
}
pub const VERSION: i64 = 3;
const IMAGES: &str = "CREATE TABLE memory_images (
    id TEXT PRIMARY KEY NOT NULL,
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    metadata TEXT NOT NULL CHECK(json_valid(metadata)),
    data BLOB NOT NULL
); CREATE INDEX memory_images_owner ON memory_images(memory_id);";
pub const APPLICATION_ID: i64 = 0x4f4d4e49;
const SCHEMA: &str = r#"
CREATE TABLE memories (
    id TEXT PRIMARY KEY NOT NULL,
    record TEXT NOT NULL CHECK (json_valid(record)),
    project_id TEXT GENERATED ALWAYS AS (json_extract(record, '$.project_id')) STORED,
    client_id TEXT GENERATED ALWAYS AS (json_extract(record, '$.client_id')) STORED,
    scope TEXT GENERATED ALWAYS AS (json_extract(record, '$.scope')) STORED,
    kind TEXT GENERATED ALWAYS AS (json_extract(record, '$.type')) STORED,
    status TEXT GENERATED ALWAYS AS (json_extract(record, '$.status')) STORED,
    task_id TEXT GENERATED ALWAYS AS (json_extract(record, '$.task_id')) STORED,
    title TEXT GENERATED ALWAYS AS (json_extract(record, '$.title')) STORED,
    summary TEXT GENERATED ALWAYS AS (json_extract(record, '$.summary')) STORED,
    body TEXT GENERATED ALWAYS AS (json_extract(record, '$.body')) STORED,
    signals TEXT GENERATED ALWAYS AS (
        coalesce(json_extract(record, '$.topics'), '') || ' ' ||
        coalesce(json_extract(record, '$.keywords'), '') || ' ' ||
        coalesce(json_extract(record, '$.triggers'), '')
    ) STORED,
    learning TEXT GENERATED ALWAYS AS (
        coalesce(json_extract(record, '$.context'), '') || ' ' ||
        coalesce(json_extract(record, '$.failure'), '') || ' ' ||
        coalesce(json_extract(record, '$.cause'), '') || ' ' ||
        coalesce(json_extract(record, '$.prevention'), '') || ' ' ||
        coalesce(json_extract(record, '$.verification'), '')
    ) STORED,
    CHECK (id = json_extract(record, '$.id'))
);
CREATE INDEX memories_scope ON memories(scope, status);
CREATE INDEX memories_project ON memories(project_id, task_id);
CREATE INDEX memories_client ON memories(client_id);
CREATE INDEX memories_kind ON memories(kind, status);
CREATE TABLE edges (
    source TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    relation TEXT NOT NULL,
    target TEXT NOT NULL,
    PRIMARY KEY (source, relation, target),
    CHECK (source <> target)
);
-- Targets may live in a different accessible store, so only source has an FK.
CREATE INDEX edges_target ON edges(target);
CREATE VIRTUAL TABLE memory_search USING fts5(
    title, summary, body, signals, learning,
    content='memories', content_rowid='rowid',
    tokenize='porter unicode61 remove_diacritics 2', prefix='2 3 4'
);
CREATE TRIGGER memories_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memory_search(rowid,title,summary,body,signals,learning)
    VALUES(new.rowid,new.title,new.summary,new.body,new.signals,new.learning);
END;
CREATE TRIGGER memories_delete AFTER DELETE ON memories BEGIN
    INSERT INTO memory_search(memory_search,rowid,title,summary,body,signals,learning)
    VALUES('delete',old.rowid,old.title,old.summary,old.body,old.signals,old.learning);
END;
CREATE TRIGGER memories_update AFTER UPDATE ON memories BEGIN
    INSERT INTO memory_search(memory_search,rowid,title,summary,body,signals,learning)
    VALUES('delete',old.rowid,old.title,old.summary,old.body,old.signals,old.learning);
    INSERT INTO memory_search(rowid,title,summary,body,signals,learning)
    VALUES(new.rowid,new.title,new.summary,new.body,new.signals,new.learning);
END;
"#;

/// Own setup locking so callers cannot accidentally open or migrate without it.
pub fn open(directory: &Path) -> Result<Connection> {
    preflight(directory)?;
    let _lock = storage::Lock::at(directory)?;
    if directory.join("project.json").exists() {
        storage::ignore_store_files(directory)?;
    }
    let path = directory.join("memory.sqlite");
    let legacy_path = directory.join("graph.json");
    // Parse legacy data before creating a new database. Invalid legacy data is
    // never silently replaced with an empty graph.
    let legacy_graph = if !path.exists() && legacy_path.exists() {
        Some(legacy(&legacy_path)?)
    } else {
        None
    };
    if !path.exists() {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path)?;
    }
    let mut connection =
        Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    connection.busy_timeout(Duration::from_secs(15))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.create_scalar_function(
        "omni_graph_write_schema",
        0,
        FunctionFlags::SQLITE_UTF8,
        |_| Ok(VERSION),
    )?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 0 {
        let tables: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            tables == 0,
            "unrecognized SQLite database {}; refusing to initialize over existing tables",
            path.display()
        );
        let graph = if let Some(graph) = legacy_graph {
            graph
        } else if legacy_path.exists() {
            legacy(&legacy_path)?
        } else {
            Graph {
                schema_version: 1,
                ..Graph::default()
            }
        };
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        tx.execute_batch(IMAGES)?;
        install_write_fences(&tx)?;
        save(&tx, &Graph::default(), &graph)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
    } else {
        validate_identity(&connection, version)?;
        if version < VERSION {
            // The filesystem lock coordinates current clients; SQLite's writer lock
            // also excludes writers from binaries that predate that lock protocol.
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let locked_version = schema_version(&tx)?;
            validate_identity(&tx, locked_version)?;
            if locked_version < VERSION {
                let backup = backup_before_migration(&path, locked_version).map_err(|error| failure(
                    "backup_failed", "Check free space and store-directory permissions, then retry; no migration was applied.", format!("pre-migration backup failed ({})", migration_failure_kind(&error))
                ))?;
                migrate(&tx, locked_version).map_err(|error| failure(
                    "migration_failed", "Keep the database and its backup; resolve the reported failure before retrying or restore the matching backup with all processes stopped.",
                    format!("migration from SQLite schema {locked_version} to {VERSION} failed; transaction rolled back; pre-migration backup: {} ({})", backup.display(), migration_failure_kind(&error))
                ))?;
            }
            tx.commit().map_err(|error| failure(
                "migration_failed",
                "Keep the database and backup, stop writers, and inspect compatibility before retrying or restoring.",
                format!("schema migration commit failed; pre-migration backup is retained beside memory.sqlite ({})", error.sqlite_error_code().map_or_else(|| "SQLite failure".to_owned(), |code| format!("{code:?}")))
            ))?;
        }
    }

    connection.pragma_update(None, "journal_mode", "WAL")?;
    // FULL keeps committed memories durable across power loss on supported local filesystems.
    connection.pragma_update(None, "synchronous", "FULL")?;
    Ok(connection)
}
/// Validate before opening a write connection or creating store housekeeping files.
pub fn preflight(directory: &Path) -> Result<()> {
    use crate::compatibility_probe::Probe;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let result = loop {
        let result = crate::compatibility_probe::inspect(directory);
        // Windows can also report access denied while a WAL file is delete-pending.
        // Retry it for normal opening, but preserve the final permission diagnosis.
        let retryable =
            result == Probe::Busy || (cfg!(windows) && result == Probe::PermissionDenied);
        if !retryable || std::time::Instant::now() >= deadline {
            break result;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    match result {
        Probe::Missing | Probe::Legacy => Ok(()),
        Probe::Schema(version) if (1..=VERSION).contains(&version) => Ok(()),
        Probe::Schema(version) => Err(failure(
            "unsupported_schema",
            "Use an executable that supports the detected schema; never downgrade the database.",
            format!(
                "unsupported SQLite schema {version}; this executable supports schema {VERSION} and migrations from 1 or 2"
            ),
        )),
        Probe::PermissionDenied => {
            anyhow::bail!("permission denied while inspecting memory.sqlite")
        }
        Probe::InvalidDatabase => anyhow::bail!(
            "memory.sqlite is not a database, is corrupt or unreadable; preserve it and recover from a verified backup"
        ),
        Probe::UnrecognizedDatabase => anyhow::bail!("memory.sqlite is not an Omni-graph database"),
        Probe::Busy => anyhow::bail!(
            "memory.sqlite changed during inspection; retry after active writers stop"
        ),
        Probe::IoError => anyhow::bail!(
            "unable to inspect memory.sqlite; check filesystem access and available temporary space"
        ),
    }
}

// SQLite diagnostics can include caller-controlled SQL or trigger text. Keep
// migration failures actionable without echoing stored content into errors.
fn migration_failure_kind(error: &anyhow::Error) -> String {
    if let Some(sqlite) = error.downcast_ref::<rusqlite::Error>() {
        return sqlite
            .sqlite_error_code()
            .map_or_else(|| "SQLite failure".to_owned(), |code| format!("{code:?}"));
    }
    if let Some(io) = error.downcast_ref::<std::io::Error>() {
        return format!("filesystem {:?}", io.kind());
    }
    "backup or migration validation failed".to_owned()
}
fn schema_version(connection: &Connection) -> Result<i64> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}
fn validate_identity(connection: &Connection, version: i64) -> Result<()> {
    if !(1..=VERSION).contains(&version) {
        return Err(failure(
            "unsupported_schema",
            "Use an executable that supports the detected schema; never downgrade the database.",
            format!(
                "unsupported SQLite schema {version}; this executable supports schema {VERSION} and migrations from 1 or 2"
            ),
        ));
    }
    let application: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    ensure!(
        application == APPLICATION_ID,
        "memory.sqlite is not an Omni-graph database"
    );
    Ok(())
}
/// Must be called under BEGIN IMMEDIATE so schema validation and writes cannot race.
pub fn write_transaction(connection: &mut Connection) -> Result<Transaction<'_>> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version = schema_version(&tx)?;
    validate_identity(&tx, version)?;
    if version != VERSION {
        return Err(failure(
            "schema_changed",
            "Reopen the store with a compatible executable before writing.",
            format!("SQLite schema changed to {version}; expected {VERSION}"),
        ));
    }
    Ok(tx)
}
fn install_write_fences(connection: &Connection) -> Result<()> {
    for table in ["memories", "edges", "memory_images"] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            connection.execute_batch(&format!(
                "CREATE TRIGGER guard_{table}_{operation} BEFORE {operation} ON {table} BEGIN SELECT CASE WHEN omni_graph_write_schema() <> {VERSION} THEN RAISE(ABORT, 'incompatible Omni-graph writer; update executable') END; END;"
            ))?;
        }
    }
    Ok(())
}
fn migrate(connection: &Connection, from: i64) -> Result<()> {
    if from == 1 {
        connection.execute_batch(IMAGES)?;
    }
    // Rename the private search table as well: old reindex/check commands must
    // fail closed instead of mutating indexes through an unguarded virtual table.
    connection.execute_batch("DROP TRIGGER memories_insert; DROP TRIGGER memories_delete; DROP TRIGGER memories_update; ALTER TABLE memory_fts RENAME TO memory_search;")?;
    let triggers = SCHEMA
        .split_once("CREATE TRIGGER memories_insert")
        .expect("schema includes FTS triggers")
        .1;
    connection.execute_batch(&format!("CREATE TRIGGER memories_insert{triggers}"))?;
    install_write_fences(connection)?;
    connection.pragma_update(None, "user_version", VERSION)?;
    Ok(())
}
fn backup_before_migration(path: &Path, version: i64) -> Result<std::path::PathBuf> {
    let directory = path.parent().context("database has no parent directory")?;
    let temporary = tempfile::NamedTempFile::new_in(directory)?;
    // A separate reader sees all committed WAL frames while the migration's
    // immediate transaction prevents another writer changing this snapshot.
    let reader = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    reader
        .backup(rusqlite::MAIN_DB, temporary.path(), None)
        .context("SQLite pre-migration backup failed; migration was not applied")?;
    let backup = Connection::open(temporary.path())?;
    // The backup is a standalone rollback-journal file, never dependent on sidecars.
    backup.pragma_update(None, "journal_mode", "DELETE")?;
    let check: String = backup.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    ensure!(
        check == "ok" && schema_version(&backup)? == version,
        "pre-migration backup validation failed"
    );
    drop(backup);
    temporary.as_file().sync_all()?;
    let destination = directory.join(format!(
        "memory.sqlite.backup-v{version}-{}",
        uuid::Uuid::new_v4()
    ));
    temporary
        .persist_noclobber(&destination)
        .context("retain pre-migration backup")?;
    #[cfg(unix)]
    fs::File::open(directory)?.sync_all()?;
    Ok(destination)
}

fn legacy(path: &Path) -> Result<Graph> {
    let graph: Graph = storage::read(path)?;
    ensure!(
        graph.schema_version == 1,
        "unsupported legacy graph schema {}",
        graph.schema_version
    );
    let mut ids = HashSet::new();
    for memory in &graph.memories {
        memory.data.validate()?;
        ensure!(
            valid_id(&memory.id, "mem") && ids.insert(&memory.id),
            "invalid or duplicate legacy memory ID"
        );
    }
    for edge in &graph.edges {
        ensure!(
            ids.contains(&edge.from) && edge.from != edge.to,
            "invalid legacy edge source"
        );
    }
    Ok(graph)
}
pub fn load(connection: &Connection) -> Result<Graph> {
    let mut memories = connection.prepare("SELECT record FROM memories ORDER BY rowid")?;
    let records = memories.query_map([], |row| row.get::<_, String>(0))?;
    let memories = records
        .map(|row| -> Result<Memory> { Ok(serde_json::from_str(&row?)?) })
        .collect::<Result<Vec<_>>>()?;
    let mut statement =
        connection.prepare("SELECT source, relation, target FROM edges ORDER BY rowid")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let edges = rows
        .map(|row| -> Result<Edge> {
            let (from, relation, to) = row?;
            Ok(Edge {
                from,
                relation: serde_json::from_value(serde_json::Value::String(relation))?,
                to,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Graph {
        schema_version: 1,
        memories,
        edges,
    })
}
/// Apply only changed rows. All memory/edge/index changes share the caller's transaction.
pub fn save(connection: &Connection, before: &Graph, after: &Graph) -> Result<()> {
    ensure!(after.schema_version == 1, "unsupported graph schema");
    let old: HashMap<_, _> = before.memories.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut seen = HashSet::new();
    let mut upsert = connection.prepare("INSERT INTO memories(id,record) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET record=excluded.record")?;
    for memory in &after.memories {
        ensure!(
            seen.insert(memory.id.as_str()),
            "duplicate memory ID {}",
            memory.id
        );
        let record = serde_json::to_string(memory)?;
        let changed = match old.get(memory.id.as_str()) {
            Some(previous) => serde_json::to_string(previous)? != record,
            None => true,
        };
        if changed {
            upsert.execute(params![memory.id, record])?;
        }
    }
    for memory in &before.memories {
        if !seen.contains(memory.id.as_str()) {
            connection.execute("DELETE FROM memories WHERE id=?1", [&memory.id])?;
        }
    }
    for edge in &before.edges {
        if !after.edges.contains(edge) {
            connection.execute(
                "DELETE FROM edges WHERE source=?1 AND relation=?2 AND target=?3",
                params![edge.from, label(&edge.relation), edge.to],
            )?;
        }
    }
    for edge in &after.edges {
        if !before.edges.contains(edge) {
            connection.execute(
                "INSERT OR IGNORE INTO edges(source,relation,target) VALUES(?1,?2,?3)",
                params![edge.from, label(&edge.relation), edge.to],
            )?;
        }
    }
    Ok(())
}
/// Escape user input into literal prefix tokens; never execute it as FTS syntax.
pub fn match_query(query: &str) -> Option<String> {
    let mut seen = HashSet::new();
    let terms: Vec<_> = query
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|term| {
            !term.is_empty()
                && !matches!(
                    term.as_str(),
                    "the" | "a" | "an" | "and" | "or" | "to" | "of" | "in" | "for"
                )
        })
        .filter(|term| seen.insert(term.clone()))
        .take(64)
        .map(|term| format!("\"{term}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}
pub fn matches(connection: &Connection, query: &str) -> Result<HashMap<String, f64>> {
    let Some(query) = match_query(query) else {
        return Ok(HashMap::new());
    };
    let mut stmt = connection.prepare("SELECT m.id, bm25(memory_search, 8.0, 3.0, 1.0, 6.0, 2.0) FROM memory_search JOIN memories m ON m.rowid=memory_search.rowid WHERE memory_search MATCH ?1 ORDER BY 2, m.id")?;
    let rows = stmt.query_map([query], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}
pub fn check(connection: &Connection) -> Result<()> {
    let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    ensure!(result == "ok", "SQLite integrity check failed: {result}");
    connection.execute(
        "INSERT INTO memory_search(memory_search, rank) VALUES('integrity-check', 1)",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn historical(directory: &Path, version: i64) -> Connection {
        let connection = Connection::open(directory.join("memory.sqlite")).unwrap();
        connection
            .execute_batch(&SCHEMA.replace("memory_search", "memory_fts"))
            .unwrap();
        if version == 2 {
            connection.execute_batch(IMAGES).unwrap();
        }
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .unwrap();
        connection.execute("INSERT INTO memories(id,record) VALUES('mem_one',?1)", [r#"{"id":"mem_one","title":"Zebra finding","scope":"project","type":"fact","metadata":{"nested":true}}"#]).unwrap();
        connection
            .execute(
                "INSERT INTO memories(id,record) VALUES('mem_two',?1)",
                [r#"{"id":"mem_two","title":"Supporting evidence"}"#],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO edges VALUES('mem_one','supported_by','mem_two')",
                [],
            )
            .unwrap();
        connection
    }
    fn backups(directory: &Path) -> Vec<std::path::PathBuf> {
        fs::read_dir(directory)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("memory.sqlite.backup-")
            })
            .collect()
    }
    fn records(connection: &Connection) -> Vec<String> {
        connection
            .prepare("SELECT record FROM memories ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    #[test]
    fn migrates_both_historical_schemas_and_backs_up_committed_wal() {
        for version in [1, 2] {
            let directory = tempfile::tempdir().unwrap();
            let old = historical(directory.path(), version);
            if version == 2 {
                old.execute("INSERT INTO memory_images VALUES('img_one','mem_one','{\"caption\":\"Reference\"}',x'010203')", []).unwrap();
            }
            let expected = records(&old);
            assert!(
                directory
                    .path()
                    .join("memory.sqlite-wal")
                    .metadata()
                    .unwrap()
                    .len()
                    > 0
            );
            let current = open(directory.path()).unwrap();
            assert_eq!(schema_version(&current).unwrap(), VERSION);
            assert_eq!(records(&current), expected);
            assert_eq!(matches(&current, "zebra").unwrap().len(), 1);
            assert_eq!(
                current
                    .query_row("SELECT count(*) FROM edges", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            let saved = backups(directory.path());
            assert_eq!(saved.len(), 1);
            let backup = Connection::open(&saved[0]).unwrap();
            assert_eq!(schema_version(&backup).unwrap(), version);
            assert_eq!(records(&backup), expected);
            assert_eq!(
                backup
                    .query_row("SELECT count(*) FROM edges", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            if version == 2 {
                assert_eq!(
                    current
                        .query_row("SELECT data FROM memory_images", [], |r| r
                            .get::<_, Vec<u8>>(0))
                        .unwrap(),
                    [1, 2, 3]
                );
                assert_eq!(
                    backup
                        .query_row("SELECT data FROM memory_images", [], |r| r
                            .get::<_, Vec<u8>>(0))
                        .unwrap(),
                    [1, 2, 3]
                );
            }
            // Reopening an already-current store does not migrate or back up again.
            open(directory.path()).unwrap();
            assert_eq!(backups(directory.path()).len(), 1);
        }
    }
    #[test]
    fn migration_waits_for_writer_and_preserves_reader_snapshot_then_fences_old_writer() {
        let directory = tempfile::tempdir().unwrap();
        let old = historical(directory.path(), 2);
        let reader = Connection::open(directory.path().join("memory.sqlite")).unwrap();
        reader.execute_batch("BEGIN").unwrap();
        assert_eq!(records(&reader).len(), 2);
        old.execute_batch("BEGIN IMMEDIATE; UPDATE memories SET record=json_set(record,'$.title','Committed during migration startup') WHERE id='mem_one';").unwrap();
        let path = directory.path().to_path_buf();
        let (send, receive) = mpsc::channel();
        let migration = std::thread::spawn(move || {
            let result = open(&path).map(|_| ());
            send.send(result).unwrap();
        });
        assert!(matches!(
            receive.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        old.execute_batch("COMMIT").unwrap();
        receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        migration.join().unwrap();
        // A WAL reader remains valid on its old snapshot until it releases it.
        assert!(records(&reader)[0].contains("Zebra finding"));
        reader.execute_batch("COMMIT").unwrap();
        assert!(records(&reader)[0].contains("Committed during migration startup"));
        for sql in [
            "UPDATE memories SET record=record WHERE id='mem_one'",
            "DELETE FROM edges WHERE source='mem_one'",
            "INSERT INTO memory_images VALUES('img_old','mem_one','{}',x'01')",
            "INSERT INTO memory_fts(memory_fts) VALUES('rebuild')",
            "INSERT INTO memory_fts(memory_fts,rank) VALUES('integrity-check',1)",
        ] {
            assert!(
                old.execute(sql, []).is_err(),
                "historical writer was allowed: {sql}"
            );
        }
        let backup = Connection::open(&backups(directory.path())[0]).unwrap();
        assert!(records(&backup)[0].contains("Committed during migration startup"));
    }
    #[test]
    fn migration_failure_rolls_back_and_retains_usable_backup() {
        let directory = tempfile::tempdir().unwrap();
        let old = historical(directory.path(), 2);
        // A conflicting trigger fails late, after the search-table rename.
        old.execute_batch(
            "CREATE TRIGGER guard_edges_DELETE BEFORE DELETE ON edges BEGIN SELECT 1; END;",
        )
        .unwrap();
        let expected = records(&old);
        let error = open(directory.path()).unwrap_err().to_string();
        assert!(error.contains("pre-migration backup"), "{error}");
        assert_eq!(schema_version(&old).unwrap(), 2);
        assert_eq!(records(&old), expected);
        old.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", [])
            .unwrap();
        assert!(old.prepare("SELECT * FROM memory_search").is_err());
        let backup = Connection::open(&backups(directory.path())[0]).unwrap();
        assert_eq!(records(&backup), expected);
        assert_eq!(schema_version(&backup).unwrap(), 2);
        old.execute_batch("DROP TRIGGER guard_edges_DELETE")
            .unwrap();
        open(directory.path()).unwrap();
        assert_eq!(schema_version(&old).unwrap(), VERSION);
    }
    #[test]
    fn future_schema_refusal_does_not_change_database_or_create_backup() {
        let directory = tempfile::tempdir().unwrap();
        let old = historical(directory.path(), 2);
        old.pragma_update(None, "user_version", VERSION + 1)
            .unwrap();
        drop(old);
        let path = directory.path().join("memory.sqlite");
        let before = fs::read(&path).unwrap();
        assert!(open(directory.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(backups(directory.path()).is_empty());
    }
    #[test]
    fn future_live_wal_refusal_preserves_every_file_and_directory_entry() {
        let directory = tempfile::tempdir().unwrap();
        let old = historical(directory.path(), 2);
        old.pragma_update(None, "user_version", VERSION + 1)
            .unwrap();
        let snapshot = || {
            let mut entries: Vec<_> = fs::read_dir(directory.path())
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), fs::read(entry.path()).unwrap())
                })
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            entries
        };
        let before = snapshot();
        let store = storage::Store {
            path: directory.path().to_path_buf(),
        };
        assert!(store.connection().is_err());
        assert_eq!(snapshot(), before);
        assert!(open(directory.path()).is_err());
        assert_eq!(snapshot(), before);
    }
    #[test]
    fn already_open_current_connection_rechecks_schema_under_writer_lock() {
        let directory = tempfile::tempdir().unwrap();
        let mut current = open(directory.path()).unwrap();
        let future = Connection::open(directory.path().join("memory.sqlite")).unwrap();
        future
            .pragma_update(None, "user_version", VERSION + 1)
            .unwrap();
        assert!(write_transaction(&mut current).is_err());
        assert!(current.is_autocommit());
    }
}
