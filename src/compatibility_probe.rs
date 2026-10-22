//! Non-mutating store inspection. SQLite opens only a disposable copy, so it
//! cannot create source WAL/SHM files or checkpoint the selected store.
//!
//! Two matching streaming copies reduce races with active writers; they are not
//! a filesystem snapshot or a lock on historical writers. A busy result asks the
//! caller to retry after writers stop. Even a successful result is advisory:
//! writers must recheck compatibility inside their own write transaction.
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Missing,
    Legacy,
    Schema(i64),
    PermissionDenied,
    InvalidDatabase,
    UnrecognizedDatabase,
    Busy,
    IoError,
}

fn io_failure(error: io::Error) -> Probe {
    // Win32 sharing/byte-range locks are transient, even though Rust maps them
    // to PermissionDenied. A snapshot can race with SQLite checkpoint/cleanup.
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return Probe::Busy;
    }
    if error.kind() == io::ErrorKind::PermissionDenied {
        Probe::PermissionDenied
    } else {
        Probe::IoError
    }
}

fn copy_file(source: &Path, target: &Path) -> io::Result<Option<[u8; 32]>> {
    let mut input = match File::open(source) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut output = File::create(target)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    Ok(Some(digest.finalize().into()))
}

fn copy_store(source: &Path, target: &Path) -> io::Result<[Option<[u8; 32]>; 2]> {
    Ok([
        copy_file(&source.join("memory.sqlite"), &target.join("memory.sqlite"))?,
        copy_file(
            &source.join("memory.sqlite-wal"),
            &target.join("memory.sqlite-wal"),
        )?,
    ])
}

/// Inspect an existing store without changing any of its files or directories.
/// No memory contents or SQLite diagnostic strings escape this boundary.
pub fn inspect(directory: &Path) -> Probe {
    // A rollback journal may be needed to recover uncommitted main-file pages.
    // Do not diagnose a copy that omits that recovery state.
    match fs::metadata(directory.join("memory.sqlite-journal")) {
        Ok(meta) if meta.len() > 0 => return Probe::Busy,
        Ok(_) => (),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return io_failure(error),
    }
    for _ in 0..3 {
        let first = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => return io_failure(error),
        };
        let second = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => return io_failure(error),
        };
        let before = match copy_store(directory, first.path()) {
            Ok(value) => value,
            Err(error) => return io_failure(error),
        };
        let after = match copy_store(directory, second.path()) {
            Ok(value) => value,
            Err(error) => return io_failure(error),
        };
        if before != after {
            continue;
        }
        if after[0].is_none() {
            if after[1].is_some() {
                return Probe::InvalidDatabase;
            }
            return match fs::metadata(directory.join("graph.json")) {
                Ok(metadata) if metadata.is_file() => Probe::Legacy,
                Ok(_) => Probe::InvalidDatabase,
                Err(error) if error.kind() == io::ErrorKind::NotFound => Probe::Missing,
                Err(error) => io_failure(error),
            };
        }
        return inspect_copy(&second.path().join("memory.sqlite"));
    }
    Probe::Busy
}

fn inspect_copy(path: &Path) -> Probe {
    let result = (|| -> rusqlite::Result<Probe> {
        // Read/write is deliberate: SQLite may recover WAL into this private
        // copy. No connection ever points at the original database.
        let connection = Connection::open(path)?;
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            return Ok(Probe::InvalidDatabase);
        }
        if application != crate::database::APPLICATION_ID {
            return Ok(Probe::UnrecognizedDatabase);
        }
        if (1..=3).contains(&version) && !known_structure(&connection, version)? {
            return Ok(Probe::InvalidDatabase);
        }
        Ok(Probe::Schema(version))
    })();
    match result {
        Ok(value) => value,
        Err(rusqlite::Error::SqliteFailure(error, _))
            if matches!(
                error.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            Probe::Busy
        }
        Err(rusqlite::Error::SqliteFailure(error, _))
            if matches!(
                error.code,
                rusqlite::ErrorCode::SystemIoFailure
                    | rusqlite::ErrorCode::DiskFull
                    | rusqlite::ErrorCode::CannotOpen
            ) =>
        {
            Probe::IoError
        }
        Err(_) => Probe::InvalidDatabase,
    }
}

/// Validate the application structure for the explicitly supported schema set.
/// Future schemas remain opaque: their names/layout may change legitimately.
/// This checks required structure, not arbitrary SQL equivalence or hostile edits.
fn known_structure(connection: &Connection, version: i64) -> rusqlite::Result<bool> {
    fn object(
        connection: &Connection,
        kind: &str,
        name: &str,
        table: &str,
    ) -> rusqlite::Result<bool> {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type=?1 AND name=?2 AND tbl_name=?3)",
            [kind, name, table],
            |row| row.get(0),
        )
    }
    fn columns(connection: &Connection, table: &str, names: &[&str]) -> rusqlite::Result<bool> {
        let mut statement = connection.prepare("SELECT name FROM pragma_table_xinfo(?1)")?;
        let actual = statement
            .query_map([table], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(names
            .iter()
            .all(|name| actual.iter().any(|value| value == name)))
    }
    let search = if version == 3 {
        "memory_search"
    } else {
        "memory_fts"
    };
    for (table, fields) in [
        (
            "memories",
            &[
                "id",
                "record",
                "project_id",
                "client_id",
                "scope",
                "kind",
                "status",
                "task_id",
                "title",
                "summary",
                "body",
                "signals",
                "learning",
            ][..],
        ),
        ("edges", &["source", "relation", "target"][..]),
        (
            search,
            &["title", "summary", "body", "signals", "learning"][..],
        ),
    ] {
        if !object(connection, "table", table, table)? || !columns(connection, table, fields)? {
            return Ok(false);
        }
    }
    for (name, table) in [
        ("memories_scope", "memories"),
        ("memories_project", "memories"),
        ("memories_client", "memories"),
        ("memories_kind", "memories"),
        ("edges_target", "edges"),
    ] {
        if !object(connection, "index", name, table)? {
            return Ok(false);
        }
    }
    for name in ["memories_insert", "memories_delete", "memories_update"] {
        if !object(connection, "trigger", name, "memories")? {
            return Ok(false);
        }
    }
    if version >= 2
        && (!object(connection, "table", "memory_images", "memory_images")?
            || !columns(
                connection,
                "memory_images",
                &["id", "memory_id", "metadata", "data"],
            )?
            || !object(connection, "index", "memory_images_owner", "memory_images")?)
    {
        return Ok(false);
    }
    if version == 3 {
        for table in ["memories", "edges", "memory_images"] {
            for operation in ["INSERT", "UPDATE", "DELETE"] {
                if !object(
                    connection,
                    "trigger",
                    &format!("guard_{table}_{operation}"),
                    table,
                )? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(directory: &Path) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect();
        files.sort();
        files
    }

    #[cfg(windows)]
    #[test]
    fn windows_lock_conflicts_are_busy_and_access_denied_remains_permission_denied() {
        for code in [32, 33] {
            assert_eq!(io_failure(io::Error::from_raw_os_error(code)), Probe::Busy);
        }
        assert_eq!(
            io_failure(io::Error::from_raw_os_error(5)),
            Probe::PermissionDenied
        );
    }

    #[test]
    fn missing_and_legacy_do_not_create_files() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        assert_eq!(inspect(&missing), Probe::Missing);
        assert!(!missing.exists());
        fs::write(root.path().join("graph.json"), b"{}").unwrap();
        let before = bytes(root.path());
        assert_eq!(inspect(root.path()), Probe::Legacy);
        assert_eq!(bytes(root.path()), before);
    }

    #[test]
    fn future_schema_in_live_wal_is_seen_without_source_changes() {
        let root = tempfile::tempdir().unwrap();
        let connection = Connection::open(root.path().join("memory.sqlite")).unwrap();
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        connection
            .pragma_update(None, "application_id", crate::database::APPLICATION_ID)
            .unwrap();
        connection.pragma_update(None, "user_version", 999).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE sample(value TEXT); INSERT INTO sample VALUES ('preserved');",
            )
            .unwrap();
        let before = bytes(root.path());
        assert!(before.iter().any(|(name, _)| name.ends_with("-wal")));
        assert_eq!(inspect(root.path()), Probe::Schema(999));
        assert_eq!(bytes(root.path()), before);
        let value: String = connection
            .query_row("SELECT value FROM sample", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "preserved");
    }

    #[test]
    fn orphan_wal_and_rollback_journal_are_not_treated_as_empty_stores() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("memory.sqlite-wal"), b"recovery data").unwrap();
        let before = bytes(root.path());
        assert_eq!(inspect(root.path()), Probe::InvalidDatabase);
        assert_eq!(bytes(root.path()), before);
        fs::write(
            root.path().join("memory.sqlite-journal"),
            b"active transaction",
        )
        .unwrap();
        let before = bytes(root.path());
        assert_eq!(inspect(root.path()), Probe::Busy);
        assert_eq!(bytes(root.path()), before);
    }

    #[test]
    fn corrupt_and_foreign_databases_are_distinct() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("memory.sqlite");
        fs::write(&path, b"this is not a database").unwrap();
        let before = bytes(root.path());
        assert_eq!(inspect(root.path()), Probe::InvalidDatabase);
        assert_eq!(bytes(root.path()), before);
        fs::remove_file(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE other(value TEXT)")
            .unwrap();
        drop(connection);
        let before = bytes(root.path());
        assert_eq!(inspect(root.path()), Probe::UnrecognizedDatabase);
        assert_eq!(bytes(root.path()), before);
    }

    #[test]
    fn identity_headers_do_not_make_an_incomplete_store_compatible() {
        for version in [1, 2, 3] {
            let root = tempfile::tempdir().unwrap();
            let connection = Connection::open(root.path().join("memory.sqlite")).unwrap();
            connection
                .pragma_update(None, "application_id", crate::database::APPLICATION_ID)
                .unwrap();
            connection
                .pragma_update(None, "user_version", version)
                .unwrap();
            drop(connection);
            let before = bytes(root.path());
            assert_eq!(inspect(root.path()), Probe::InvalidDatabase);
            assert_eq!(bytes(root.path()), before);
        }
    }

    #[test]
    fn missing_required_structures_and_each_writer_guard_are_rejected() {
        let mut removals = vec![
            "DROP INDEX memories_scope".to_owned(),
            "DROP TRIGGER memories_insert".to_owned(),
            "DROP INDEX memory_images_owner".to_owned(),
            "ALTER TABLE memory_images RENAME COLUMN metadata TO missing_metadata".to_owned(),
        ];
        for table in ["memories", "edges", "memory_images"] {
            for operation in ["INSERT", "UPDATE", "DELETE"] {
                removals.push(format!("DROP TRIGGER guard_{table}_{operation}"));
            }
        }
        for sql in removals {
            let root = tempfile::tempdir().unwrap();
            let connection = crate::database::open(root.path()).unwrap();
            assert_eq!(inspect(root.path()), Probe::Schema(3));
            connection.execute_batch(&sql).unwrap();
            drop(connection);
            let before = bytes(root.path());
            assert_eq!(inspect(root.path()), Probe::InvalidDatabase, "{sql}");
            assert_eq!(bytes(root.path()), before, "{sql}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_is_structured_without_source_changes() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("memory.sqlite");
        fs::write(&path, b"private").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        // Root can bypass mode bits; exercise the permission case only when
        // this process actually lacks access rather than assert a false result.
        if File::open(&path).is_err() {
            assert_eq!(inspect(root.path()), Probe::PermissionDenied);
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"private");
    }
}
