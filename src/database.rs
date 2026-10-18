//! SQLite persistence. JSON is retained per row for lossless interchange; generated
//! columns, ordinary indexes, FTS5 and edges are maintained transactionally.
use crate::{model::*, storage};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, TransactionBehavior, params};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::Duration,
};

const VERSION: i64 = 1;
const APPLICATION_ID: i64 = 0x4f4d4e49;
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
CREATE VIRTUAL TABLE memory_fts USING fts5(
    title, summary, body, signals, learning,
    content='memories', content_rowid='rowid',
    tokenize='porter unicode61 remove_diacritics 2', prefix='2 3 4'
);
CREATE TRIGGER memories_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memory_fts(rowid,title,summary,body,signals,learning)
    VALUES(new.rowid,new.title,new.summary,new.body,new.signals,new.learning);
END;
CREATE TRIGGER memories_delete AFTER DELETE ON memories BEGIN
    INSERT INTO memory_fts(memory_fts,rowid,title,summary,body,signals,learning)
    VALUES('delete',old.rowid,old.title,old.summary,old.body,old.signals,old.learning);
END;
CREATE TRIGGER memories_update AFTER UPDATE ON memories BEGIN
    INSERT INTO memory_fts(memory_fts,rowid,title,summary,body,signals,learning)
    VALUES('delete',old.rowid,old.title,old.summary,old.body,old.signals,old.learning);
    INSERT INTO memory_fts(rowid,title,summary,body,signals,learning)
    VALUES(new.rowid,new.title,new.summary,new.body,new.signals,new.learning);
END;
"#;

/// Caller holds the short-lived store filesystem lock during setup/migration.
pub fn open(directory: &Path) -> Result<Connection> {
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
        save(&tx, &Graph::default(), &graph)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
    } else {
        ensure!(
            version == VERSION,
            "unsupported SQLite schema {version}; this version supports {VERSION}"
        );
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        ensure!(
            application == APPLICATION_ID,
            "memory.sqlite is not an Omni-graph database"
        );
    }
    connection.pragma_update(None, "journal_mode", "WAL")?;
    // FULL keeps committed memories durable across power loss on supported local filesystems.
    connection.pragma_update(None, "synchronous", "FULL")?;
    Ok(connection)
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
    let mut stmt = connection.prepare("SELECT m.id, bm25(memory_fts, 8.0, 3.0, 1.0, 6.0, 2.0) FROM memory_fts JOIN memories m ON m.rowid=memory_fts.rowid WHERE memory_fts MATCH ?1 ORDER BY 2, m.id")?;
    let rows = stmt.query_map([query], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}
pub fn check(connection: &Connection) -> Result<()> {
    let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    ensure!(result == "ok", "SQLite integrity check failed: {result}");
    connection.execute(
        "INSERT INTO memory_fts(memory_fts, rank) VALUES('integrity-check', 1)",
        [],
    )?;
    Ok(())
}
