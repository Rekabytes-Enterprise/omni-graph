use crate::{
    cli::*,
    integrations,
    model::*,
    retrieval,
    storage::{self, *},
    tui,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    path::Path,
};

fn emit(value: impl serde::Serialize, json_output: bool) -> Result<()> {
    let value = serde_json::to_value(value)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else if let Some(items) = value.as_array() {
        if items.is_empty() {
            println!("No results.");
        }
        for item in items {
            if let Some(memory) = item.get("memory") {
                print_memory(memory);
            } else if item.get("title").is_some() {
                print_memory(item);
            } else {
                println!("{}", serde_json::to_string_pretty(item)?);
            }
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}
fn print_memory(m: &Value) {
    println!(
        "{}  [{} / {}] {}",
        m["id"].as_str().unwrap_or(""),
        m["scope"].as_str().unwrap_or(""),
        m["type"].as_str().unwrap_or(""),
        m["title"].as_str().unwrap_or("")
    );
    if let Some(summary) = m["summary"].as_str().filter(|s| !s.is_empty()) {
        println!("  {summary}");
    }
}
pub fn input(path: &Path) -> Result<Value> {
    let mut text = String::new();
    if path == Path::new("-") {
        io::stdin().read_to_string(&mut text)?;
    } else {
        text =
            fs::read_to_string(path).with_context(|| format!("read input {}", path.display()))?;
    }
    serde_json::from_str(&text).context("input must be valid JSON")
}
pub fn run(cli: Cli) -> Result<()> {
    let start = cli.directory.unwrap_or(std::env::current_dir()?);
    let home = std::path::absolute(storage::home()?)?;
    let output = cli.json;
    let command = match cli.command {
        Some(command) => command,
        None => {
            ensure!(
                !output,
                "choose an explicit command with --json, such as graph or list"
            );
            let Some(command) = crate::ui::choose()? else {
                return Ok(());
            };
            command
        }
    };
    match command {
        Command::Install => return crate::install::run(&home, output),
        Command::Update {
            id: None,
            file: None,
        } => return crate::update::run(&home, output),
        Command::Init(args) => return init(&start, &home, args, output),
        Command::Client { command } => return clients(&home, command, output),
        Command::Project {
            command: ProjectCommand::List,
        } => return emit(registry(&home)?.projects, output),
        _ => (),
    }
    let workspace = Workspace::open(&start, home)?;
    match command {
        Command::Setup { agent } => {
            let _lock = Lock::at(&workspace.root.join(".omni-graph"))?;
            emit(integrations::install(&workspace.root, &agent)?, output)
        }
        Command::Project { command } => project(&workspace, command, output),
        Command::Image { command } => image_command(&workspace, command, output),
        Command::Remember(args) => {
            let mut images: Vec<crate::attachments::ImageInput> = vec![];
            let mut request_key: Option<String> = None;
            let (mut data, links): (MemoryInput, Vec<CreateLink>) = if let Some(file) = args.file {
                let mut value = input(&file)?;
                let object = value
                    .as_object_mut()
                    .context("memory must be a JSON object")?;
                request_key = object
                    .remove("idempotency_key")
                    .map(serde_json::from_value)
                    .transpose()?;
                let links = object
                    .remove("links")
                    .map(serde_json::from_value)
                    .transpose()?;
                if let Some(value) = object.remove("images") {
                    images = serde_json::from_value(value)?;
                }
                (serde_json::from_value(value)?, links.unwrap_or_default())
            } else {
                (MemoryInput::default(), vec![])
            };
            if let Some(v) = args.title {
                data.title = v;
            }
            if let Some(v) = args.body {
                data.body = v;
            }
            if let Some(v) = args.summary {
                data.summary = v;
            }
            if let Some(v) = args.kind {
                data.kind = v;
            }
            if let Some(v) = args.scope {
                data.scope = v;
            }
            if let Some(v) = args.task {
                data.task_id = Some(v);
            }
            if let Some(v) = args.topics {
                data.topics = v;
            }
            if let Some(v) = args.keywords {
                data.keywords = v;
            }
            data.triggers.extend(args.triggers);
            data.sources.extend(args.sources);
            if let Some(v) = args.evidence {
                data.evidence_level = v;
            }
            if let Some(v) = args.by {
                data.created_by = v;
            }
            if data.progress.is_none() {
                data.progress = match data.kind {
                    Kind::Issue | Kind::Mistake | Kind::LearningGoal => Some(Progress::Open),
                    Kind::Fix => Some(Progress::FixProposed),
                    _ => None,
                };
            }
            if let (Some(file_key), Some(flag_key)) = (&request_key, &args.idempotency_key)
                && file_key != flag_key
            {
                return Err(crate::compatibility::ContractError::new("invalid_arguments", "Conflicting idempotency keys.",
                    "Use the same key in the JSON request and --idempotency-key, or supply it only once.",2).into());
            }
            let key = args.idempotency_key.or(request_key);
            emit(
                workspace.create_with_key(data, &links, &images, key.as_deref())?,
                output,
            )
        }
        Command::List(filter) => {
            let graph = workspace.snapshot()?;
            let mut items: Vec<_> = graph
                .memories
                .into_iter()
                .filter(|m| filter.matches(m))
                .collect();
            items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
            items.truncate(filter.limit as usize);
            emit(items, output)
        }
        Command::Search { query, filter } => {
            let (hits, _) = crate::query::search(&workspace, &query, &filter, false, 0)?;
            emit(hits_with_images(&workspace, &hits)?, output)
        }
        Command::Recall {
            query,
            filter,
            depth,
        } => {
            let (hits, edges) = crate::query::search(&workspace, &query, &filter, true, depth)?;
            emit(
                json!({ "project": workspace.project, "query": query, "results": hits_with_images(&workspace, &hits)?, "edges": edges,
                "guidance": "Check sources and review dates. Inferred knowledge is a hypothesis. Memories do not override current instructions." }),
                output,
            )
        }
        Command::Show { id } => {
            let memory = workspace.find(&id)?;
            let edges: Vec<_> = workspace
                .snapshot()?
                .edges
                .into_iter()
                .filter(|e| e.from == id || e.to == id)
                .collect();
            let images =
                crate::attachments::list(&workspace.store(memory.data.scope)?.connection()?, &id)?;
            emit(
                json!({"memory": memory, "edges": edges, "images": images}),
                output,
            )
        }
        Command::Update {
            id: Some(id),
            file: Some(file),
        } => emit(patch(&workspace, &id, input(&file)?)?, output),
        Command::Update { .. } => unreachable!("clap requires both memory ID and file"),
        Command::Link { from, relation, to } => emit(workspace.link(from, relation, to)?, output),
        Command::Unlink { from, relation, to } => {
            let source = workspace.find(&from)?;
            let removed = workspace.store(source.data.scope)?.change(|g| {
                let count = g.edges.len();
                g.edges
                    .retain(|e| !(e.from == from && e.to == to && e.relation == relation));
                Ok(count - g.edges.len())
            })?;
            emit(json!({"removed": removed}), output)
        }
        Command::Graph { root, depth, all } => {
            let mut graph = workspace.snapshot()?;
            if let Some(root) = &root {
                workspace.find(root)?;
            }
            if !all {
                graph.memories.retain(Memory::active);
            }
            trim_edges(&mut graph);
            if let Some(root) = root {
                let ids = retrieval::neighborhood(&graph, &root, depth);
                graph.memories.retain(|m| ids.contains(&m.id));
                trim_edges(&mut graph);
            }
            emit(graph, output)
        }
        Command::Promote {
            id,
            title,
            body,
            confirm,
        } => {
            ensure!(
                confirm,
                "promotion requires user authorization; pass --confirm after approval"
            );
            let original = workspace.find(&id)?;
            ensure!(
                original.data.scope != Scope::Global,
                "memory is already global"
            );
            // Only carry fields explicitly supplied by the user plus safe classification.
            let data = MemoryInput {
                title,
                body,
                kind: original.data.kind,
                scope: Scope::Global,
                sources: vec![format!("omni-graph:{id}")],
                created_by: original.data.created_by,
                evidence_level: Evidence::Reported,
                ..MemoryInput::default()
            };
            emit(workspace.create(data)?, output)
        }
        Command::Review => emit(review(&workspace.snapshot()?), output),
        Command::Export { output: path } => {
            emit(crate::transfer::export(&workspace, &path)?, output)
        }
        Command::Import { file } => emit(
            crate::transfer::import(&workspace, serde_json::from_value(input(&file)?)?)?,
            output,
        ),
        Command::Doctor => emit(doctor(&workspace)?, output),
        Command::Reindex => {
            for store in workspace.stores()? {
                store.reindex()?;
            }
            emit(json!({"rebuilt": true, "engine": "sqlite_fts5"}), output)
        }
        Command::Tui => {
            ensure!(
                !output,
                "TUI cannot emit JSON; use graph, list, or recall with --json"
            );
            tui::run(&workspace)
        }
        Command::Web(args) => {
            ensure!(
                !output,
                "Web UI cannot emit JSON; use graph, list, or recall with --json"
            );
            crate::web::run(&workspace, args)
        }
        Command::Compatibility { .. }
        | Command::Install
        | Command::Init(_)
        | Command::Client { .. } => unreachable!(),
    }
}
fn init(start: &Path, home: &Path, args: Init, output: bool) -> Result<()> {
    let root = start
        .canonicalize()
        .context("initialization directory must exist")?;
    ensure!(root.is_dir(), "project root must be a directory");
    ensure!(
        std::path::absolute(root.join(".omni-graph"))? != home,
        "project memory cannot also be the global store; select another project directory or OMNI_GRAPH_HOME"
    );
    integrations::targets(&args.agent)?;
    if let Ok(existing) = find_root(&root) {
        ensure!(
            existing == root,
            "already inside project {}; initialize there",
            existing.display()
        );
    }
    if let Some(client) = &args.client {
        Workspace::check_client(home, client)?;
    }
    let directory = root.join(".omni-graph");
    let lock = Lock::at(&directory)?;
    let path = directory.join("project.json");
    let profile = if path.exists() {
        let existing: Project = read(&path)?;
        ensure!(
            args.client.is_none() || args.client == existing.client_id,
            "init cannot change project ownership"
        );
        existing
    } else {
        let name = args.name.unwrap_or_else(|| {
            root.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into()
        });
        ensure!(!name.trim().is_empty(), "project name is required");
        let profile = Project {
            schema_version: 1,
            id: id("project"),
            name,
            client_id: args.client,
            kind: args.kind,
            description: args.description,
            repository: None,
            technologies: vec![],
            status: "active".into(),
        };
        write(&path, &profile)?;
        profile
    };
    ignore_store_files(&directory)?;
    let installed = integrations::install(&root, &args.agent)?;
    drop(lock);
    Store { path: directory }.load()?;
    register(home, &profile, &root)?;
    emit(
        json!({"project": profile, "root": root, "agents": installed}),
        output,
    )
}
fn clients(home: &Path, command: ClientCommand, output: bool) -> Result<()> {
    match command {
        ClientCommand::List => emit(registry(home)?.clients, output),
        ClientCommand::Add {
            name,
            id: existing_id,
        } => {
            ensure!(!name.trim().is_empty(), "client name is required");
            let _lock = Lock::at(home)?;
            let mut reg = registry(home)?;
            ensure!(
                !reg.clients
                    .iter()
                    .any(|c| c.name.trim().eq_ignore_ascii_case(name.trim())),
                "client name already registered; use client list"
            );
            let client_id = existing_id.unwrap_or_else(|| id("client"));
            ensure!(valid_id(&client_id, "client"), "invalid client ID");
            ensure!(
                !reg.clients.iter().any(|c| c.id == client_id),
                "client ID already registered"
            );
            let client = Client {
                id: client_id,
                name,
            };
            reg.clients.push(client.clone());
            write(&home.join("registry.json"), &reg)?;
            emit(client, output)
        }
    }
}
fn hits_with_images(workspace: &Workspace, hits: &[retrieval::Hit]) -> Result<Vec<Value>> {
    let mut connections = std::collections::BTreeMap::new();
    hits.iter()
        .map(|hit| {
            let store = workspace.store(hit.memory.data.scope)?;
            if !connections.contains_key(&store.path) {
                connections.insert(store.path.clone(), store.connection()?);
            }
            let mut value = serde_json::to_value(hit)?;
            value["images"] = serde_json::to_value(crate::attachments::list(
                &connections[&store.path],
                &hit.memory.id,
            )?)?;
            Ok(value)
        })
        .collect()
}
fn image_command(workspace: &Workspace, command: ImageCommand, output: bool) -> Result<()> {
    use crate::attachments::{self, ImageInput};
    match command {
        ImageCommand::Add { id, file, caption } => {
            let memory = workspace.find(&id)?;
            let image = ImageInput { file, caption }.prepare(&id)?;
            workspace.store(memory.data.scope)?.change_with_images(
                std::slice::from_ref(&image),
                |g| {
                    g.memories
                        .iter_mut()
                        .find(|m| m.id == id)
                        .context("memory disappeared")?
                        .updated_at = now();
                    Ok(())
                },
            )?;
            emit(image.metadata, output)
        }
        ImageCommand::List { id } => {
            let memory = workspace.find(&id)?;
            emit(
                attachments::list(&workspace.store(memory.data.scope)?.connection()?, &id)?,
                output,
            )
        }
        ImageCommand::Export {
            id,
            image,
            output: path,
        } => {
            use std::io::Write;
            let image = attachments::read(workspace, &id, &image)?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&path)?.write_all(&image.data)?;
            emit(json!({"output":path,"image":image.metadata}), output)
        }
        ImageCommand::Remove { id, image } => {
            let memory = workspace.find(&id)?;
            let mut connection = workspace.store(memory.data.scope)?.connection()?;
            let tx = crate::database::write_transaction(&mut connection)?;
            ensure!(
                tx.execute(
                    "DELETE FROM memory_images WHERE memory_id=?1 AND id=?2",
                    rusqlite::params![id, image]
                )? == 1,
                "image not found for this memory"
            );
            let before = crate::database::load(&tx)?;
            let mut after = before.clone();
            after
                .memories
                .iter_mut()
                .find(|m| m.id == id)
                .context("memory disappeared")?
                .updated_at = now();
            crate::database::save(&tx, &before, &after)?;
            tx.commit()?;
            emit(json!({"removed":image}), output)
        }
    }
}
fn project(workspace: &Workspace, command: ProjectCommand, output: bool) -> Result<()> {
    match command {
        ProjectCommand::Show => emit(&workspace.project, output),
        ProjectCommand::List => emit(registry(&workspace.home)?.projects, output),
        ProjectCommand::Configure {
            name,
            description,
            repository,
            technologies,
            status,
        } => {
            let lock = Lock::at(&workspace.root.join(".omni-graph"))?;
            let path = workspace.root.join(".omni-graph/project.json");
            let mut profile: Project = read(&path)?;
            if let Some(name) = name {
                ensure!(!name.trim().is_empty(), "project name cannot be empty");
                profile.name = name;
            }
            if let Some(v) = description {
                profile.description = v;
            }
            if let Some(v) = repository {
                profile.repository = Some(v);
            }
            if let Some(v) = technologies {
                profile.technologies = v;
            }
            if let Some(v) = status {
                ensure!(
                    ["active", "paused", "completed", "archived"].contains(&v.as_str()),
                    "invalid project status"
                );
                profile.status = v;
            }
            write(&path, &profile)?;
            drop(lock);
            register(&workspace.home, &profile, &workspace.root)?;
            emit(profile, output)
        }
    }
}
pub fn patch(workspace: &Workspace, id: &str, patch: Value) -> Result<Memory> {
    let original = workspace.find(id)?;
    let object = patch.as_object().context("patch must be a JSON object")?;
    ensure!(!object.is_empty(), "patch cannot be empty");
    for key in object.keys() {
        ensure!(
            ![
                "id",
                "scope",
                "project_id",
                "client_id",
                "created_at",
                "updated_at",
                "verified_at",
                "created_by",
                "task_id"
            ]
            .contains(&key.as_str()),
            "field {key} is immutable"
        );
    }
    workspace.store(original.data.scope)?.change(|g| {
        let index = g
            .memories
            .iter()
            .position(|m| m.id == id)
            .context("memory disappeared")?;
        let memory = &mut g.memories[index];
        let old_data = memory.data.clone();
        let mut data = serde_json::to_value(&memory.data)?;
        for (key, value) in object {
            data.as_object_mut()
                .unwrap()
                .insert(key.clone(), value.clone());
        }
        if object.contains_key("body")
            && !object.contains_key("summary")
            && old_data.summary == old_data.body.chars().take(200).collect::<String>()
        {
            data["summary"] = json!(
                data["body"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect::<String>()
            );
        }
        let knowledge_changed = object.keys().any(|k| {
            ![
                "status",
                "progress",
                "review_after",
                "expires_at",
                "visibility",
            ]
            .contains(&k.as_str())
        });
        // A changed claim needs fresh evidence; updating status alone preserves its evidence.
        if knowledge_changed
            && old_data.evidence_level == Evidence::Verified
            && !object.contains_key("evidence_level")
        {
            data["evidence_level"] = json!("inferred");
            if data["progress"] == json!("verified") {
                data["progress"] = json!("fixed");
            }
        }
        let data: MemoryInput = serde_json::from_value(data).context("invalid memory patch")?;
        data.validate()?;
        if data.status == Status::Active {
            ensure!(
                !g.memories.iter().any(|m| m.id != id
                    && m.data.scope == data.scope
                    && m.data.task_id == data.task_id
                    && m.data.status == Status::Active
                    && m.data.title.trim().eq_ignore_ascii_case(data.title.trim())),
                "an active memory with this title already exists; update that memory instead"
            );
        }
        if data.status == Status::Superseded {
            ensure!(
                g.edges
                    .iter()
                    .any(|e| e.relation == Relation::Supersedes && e.to == id),
                "use link <new> supersedes <old> to supersede a memory"
            );
        }
        if data.status == Status::Active {
            ensure!(
                !g.edges
                    .iter()
                    .any(|e| e.relation == Relation::Supersedes && e.to == id),
                "unlink supersedes before reactivating this memory"
            );
        }
        let stamp = now();
        let memory = &mut g.memories[index];
        memory.verified_at = if data.evidence_level == Evidence::Verified {
            if knowledge_changed || old_data.evidence_level != Evidence::Verified {
                Some(stamp)
            } else {
                memory.verified_at
            }
        } else {
            None
        };
        memory.data = data;
        memory.updated_at = stamp;
        Ok(memory.clone())
    })
}
fn trim_edges(graph: &mut Graph) {
    let ids: std::collections::HashSet<_> = graph.memories.iter().map(|m| &m.id).collect();
    graph
        .edges
        .retain(|e| ids.contains(&e.from) && ids.contains(&e.to));
}
fn review(graph: &Graph) -> Value {
    let active: Vec<_> = graph.memories.iter().filter(|m| m.active()).collect();
    let due: Vec<_> = graph
        .memories
        .iter()
        .filter(|m| {
            m.data.status == Status::Active
                && (m.due() || m.data.expires_at.is_some_and(|t| t <= now()))
        })
        .collect();
    let unresolved: Vec<_> = active
        .iter()
        .filter(|m| m.data.progress.is_some_and(|p| p != Progress::Verified))
        .collect();
    let weak_lessons: Vec<_> = active
        .iter()
        .filter(|m| {
            m.data.kind == Kind::Lesson
                && (m.data.triggers.is_empty() || m.data.prevention.is_empty())
        })
        .collect();
    let mut causes: HashMap<String, Vec<&Memory>> = HashMap::new();
    for m in &active {
        if m.data.kind == Kind::Mistake
            && let Some(cause) = &m.data.cause
        {
            let cause = cause
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            if !cause.is_empty() {
                causes.entry(cause).or_default().push(m);
            }
        }
    }
    let mut repeated: Vec<_> = causes.into_iter().filter(|(_, m)| m.len() > 1).collect();
    repeated.sort_by(|a, b| a.0.cmp(&b.0));
    json!({"review_due": due, "unresolved": unresolved, "lessons_missing_triggers_or_prevention": weak_lessons,
        "repeated_causes": repeated.into_iter().map(|(cause, memories)| json!({"cause": cause, "memories": memories})).collect::<Vec<_>>()})
}
fn doctor(workspace: &Workspace) -> Result<Value> {
    let graph = workspace.snapshot()?;
    let mut issues = vec![];
    let mut ids = std::collections::HashSet::new();
    for store in workspace.stores()? {
        if let Err(error) = store.check() {
            issues.push(format!("{}: {error:#}", store.path.display()));
        }
        for m in store.load()?.memories {
            if !ids.insert(m.id.clone()) {
                issues.push(format!("duplicate memory ID: {}", m.id));
            }
            if let Err(error) = m.data.validate() {
                issues.push(format!("{}: {error}", m.id));
            }
            if matches!(m.data.scope, Scope::Project | Scope::Task)
                && m.project_id.as_ref() != Some(&workspace.project.id)
            {
                issues.push(format!("{}: wrong project ownership", m.id));
            }
        }
    }
    for store in workspace.stores()? {
        for edge in store.load()?.edges {
            if !ids.contains(&edge.from) || !ids.contains(&edge.to) {
                issues.push(format!("dangling edge: {} → {}", edge.from, edge.to));
            }
        }
    }
    Ok(
        json!({"ok": issues.is_empty(), "issues": issues, "project": workspace.project,
        "storage": {"engine": "sqlite", "search": "fts5", "sqlite_version": rusqlite::version(), "file": "memory.sqlite"},
        "memories": graph.memories.len(), "edges": graph.edges.len(), "agents": integrations::inspect(&workspace.root)}),
    )
}
