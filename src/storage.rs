use crate::{database, model::*};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use rusqlite::{Connection, TransactionBehavior};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

pub fn home() -> Result<PathBuf> {
    if let Some(p) = env::var_os("OMNI_GRAPH_HOME") {
        return Ok(PathBuf::from(p));
    }
    let base = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .context("cannot locate home directory; set OMNI_GRAPH_HOME")?;
    Ok(PathBuf::from(base).join(".omni-graph"))
}
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}
pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("missing parent directory")?;
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}
pub struct Lock {
    _file: File,
}
impl Lock {
    pub fn at(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join(".lock"))?;
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }
}
pub fn registry(home: &Path) -> Result<Registry> {
    let path = home.join("registry.json");
    if path.exists() {
        read(&path)
    } else {
        Ok(Registry::default())
    }
}
pub fn register(home: &Path, project: &Project, root: &Path) -> Result<()> {
    let _lock = Lock::at(home)?;
    let mut reg = registry(home)?;
    reg.projects.retain(|p| p.id != project.id);
    reg.projects.push(Registration {
        id: project.id.clone(),
        name: project.name.clone(),
        path: root.display().to_string(),
    });
    write(&home.join("registry.json"), &reg)
}
pub fn find_root(start: &Path) -> Result<PathBuf> {
    let start = start
        .canonicalize()
        .context("project directory does not exist")?;
    for ancestor in start.ancestors() {
        if ancestor.join(".omni-graph/project.json").is_file() {
            return Ok(ancestor.to_path_buf());
        }
    }
    bail!("no Omni-graph project found; run `omni-graph init` in the project root")
}
#[derive(Debug, Clone)]
pub struct Store {
    pub path: PathBuf,
}
pub fn ignore_store_files(directory: &Path) -> Result<()> {
    let path = directory.join(".gitignore");
    let mut text = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let original = text.clone();
    for entry in [
        "/graph.json",
        "/memory.sqlite",
        "/memory.sqlite-wal",
        "/memory.sqlite-shm",
        "/memory.sqlite-journal",
        "/.lock",
    ] {
        if !text.lines().any(|line| line.trim() == entry) {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(entry);
            text.push('\n');
        }
    }
    if text != original {
        fs::write(path, text)?;
    }
    Ok(())
}
impl Store {
    fn connection(&self) -> Result<Connection> {
        // File locking coordinates one-time schema creation/legacy migration.
        // Normal writes use SQLite's own transactions and busy timeout.
        let _lock = Lock::at(&self.path)?;
        if self.path.join("project.json").exists() {
            ignore_store_files(&self.path)?;
        }
        database::open(&self.path).with_context(|| format!("memory store {}", self.path.display()))
    }
    pub fn load(&self) -> Result<Graph> {
        Ok(self.snapshot(None)?.0)
    }
    fn snapshot(&self, query: Option<&str>) -> Result<(Graph, HashMap<String, f64>)> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let graph = database::load(&tx)?;
        let ranks = if let Some(query) = query {
            database::matches(&tx, query)?
        } else {
            HashMap::new()
        };
        tx.commit()?;
        Ok((graph, ranks))
    }
    pub fn change<T>(&self, f: impl FnOnce(&mut Graph) -> Result<T>) -> Result<T> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = database::load(&tx)?;
        let mut graph = before.clone();
        let value = f(&mut graph)?;
        database::save(&tx, &before, &graph)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn check(&self) -> Result<()> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        database::check(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn reindex(&self) -> Result<()> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", [])?;
        tx.commit()?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub home: PathBuf,
    pub project: Project,
}
impl Workspace {
    pub fn open(start: &Path, home: PathBuf) -> Result<Self> {
        let root = find_root(start)?;
        let project: Project = read(&root.join(".omni-graph/project.json"))?;
        ensure!(project.schema_version == 1, "unsupported project schema");
        if let Some(id) = &project.client_id {
            Self::check_client(&home, id)?;
        }
        Ok(Self {
            root,
            home,
            project,
        })
    }
    pub fn check_client(home: &Path, id: &str) -> Result<()> {
        ensure!(valid_id(id, "client"), "invalid client ID");
        ensure!(
            registry(home)?.clients.iter().any(|c| c.id == id),
            "unknown client {id}; use `client list` or `client add`"
        );
        Ok(())
    }
    pub fn store(&self, scope: Scope) -> Result<Store> {
        let path = match scope {
            Scope::Global => self.home.clone(),
            Scope::Client => {
                let client = self
                    .project
                    .client_id
                    .as_ref()
                    .context("project has no client")?;
                Self::check_client(&self.home, client)?;
                self.home.join("clients").join(client)
            }
            Scope::Project | Scope::Task => self.root.join(".omni-graph"),
        };
        Ok(Store { path })
    }
    pub fn stores(&self) -> Result<Vec<Store>> {
        let mut stores = vec![self.store(Scope::Global)?];
        if self.project.client_id.is_some() {
            stores.push(self.store(Scope::Client)?);
        }
        stores.push(self.store(Scope::Project)?);
        Ok(stores)
    }
    pub fn snapshot(&self) -> Result<Graph> {
        Ok(self.collect_snapshot(None)?.0)
    }
    pub fn indexed_snapshot(&self, query: &str) -> Result<(Graph, HashMap<String, f64>)> {
        self.collect_snapshot(Some(query))
    }
    fn collect_snapshot(&self, query: Option<&str>) -> Result<(Graph, HashMap<String, f64>)> {
        let mut ranks = HashMap::new();
        let mut graph = Graph {
            schema_version: 1,
            ..Graph::default()
        };
        for store in self.stores()? {
            let (source, matches) = store.snapshot(query)?;
            ranks.extend(matches);
            graph
                .memories
                .extend(source.memories.into_iter().filter(|m| self.visible(m)));
            graph.edges.extend(source.edges);
        }
        let ids: std::collections::HashSet<_> =
            graph.memories.iter().map(|m| m.id.clone()).collect();
        graph
            .edges
            .retain(|e| ids.contains(&e.from) && ids.contains(&e.to));
        ranks.retain(|id, _| ids.contains(id));
        Ok((graph, ranks))
    }
    fn visible(&self, m: &Memory) -> bool {
        match m.data.scope {
            Scope::Global => m.project_id.is_none() && m.client_id.is_none(),
            Scope::Client => {
                m.project_id.is_none()
                    && m.client_id.is_some()
                    && m.client_id == self.project.client_id
            }
            Scope::Project | Scope::Task => {
                m.project_id.as_ref() == Some(&self.project.id)
                    && m.client_id == self.project.client_id
            }
        }
    }
    pub fn find(&self, id: &str) -> Result<Memory> {
        self.snapshot()?
            .memories
            .into_iter()
            .find(|m| m.id == id)
            .with_context(|| format!("memory {id} not found in this project's accessible scopes"))
    }
    pub fn create(&self, mut data: MemoryInput) -> Result<Memory> {
        data.validate()?;
        ensure!(
            data.status != Status::Superseded,
            "create an active memory, then link its successor with supersedes"
        );
        if data.summary.is_empty() {
            data.summary = data.body.chars().take(200).collect();
        }
        let stamp = now();
        let memory = Memory {
            id: id("mem"),
            project_id: matches!(data.scope, Scope::Project | Scope::Task)
                .then(|| self.project.id.clone()),
            client_id: (data.scope != Scope::Global)
                .then(|| self.project.client_id.clone())
                .flatten(),
            created_at: stamp,
            updated_at: stamp,
            verified_at: (data.evidence_level == Evidence::Verified).then_some(stamp),
            data,
        };
        self.store(memory.data.scope)?.change(|g| {
            ensure!(
                !g.memories.iter().any(|m| m.data.scope == memory.data.scope
                    && m.project_id == memory.project_id
                    && m.data.task_id == memory.data.task_id
                    && m.data.status == Status::Active
                    && m.data
                        .title
                        .trim()
                        .eq_ignore_ascii_case(memory.data.title.trim())),
                "an active memory with this title already exists; search and update it"
            );
            g.memories.push(memory.clone());
            Ok(())
        })?;
        Ok(memory)
    }
}
