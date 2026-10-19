use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        fs::create_dir(fixture.root()).unwrap();
        fixture.ok(&["init", "--name", "Booking Portal"]);
        fixture
    }
    fn root(&self) -> std::path::PathBuf {
        self.temp.path().join("project")
    }
    fn cmd(&self, root: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_omni-graph"));
        cmd.env("OMNI_GRAPH_HOME", self.temp.path().join("home"))
            .env("HOME", self.temp.path().join("shell-home"))
            .env("USERPROFILE", self.temp.path().join("shell-home"))
            .env("SHELL", "/bin/bash")
            .arg("--directory")
            .arg(root)
            .args(args)
            .arg("--json");
        cmd
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cmd(&self.root(), args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        parse(self.run(args))
    }
    fn fail(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "unexpected success");
        String::from_utf8(out.stderr).unwrap()
    }
    fn stdin(&self, args: &[&str], input: Value) -> Value {
        let mut cmd = self.cmd(&self.root(), args);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        parse(child.wait_with_output().unwrap())
    }
    fn memory(&self, input: Value) -> Value {
        self.stdin(&["remember", "--file", "-"], input)
    }
}
fn parse(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("invalid JSON: {}", String::from_utf8_lossy(&output.stdout)))
}
fn id(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

#[test]
fn initialization_is_idempotent_and_preserves_existing_instructions() {
    let f = Fixture::new();
    let initial = f.ok(&["project", "show"]);
    let path = f.root().join("AGENTS.md");
    let current = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!("# Custom instructions\nKeep this rule.\n{current}\nEnd rule.\n"),
    )
    .unwrap();
    f.ok(&["init"]);
    f.ok(&["setup"]);
    let text = fs::read_to_string(path).unwrap();
    assert!(text.starts_with("# Custom instructions\nKeep this rule.\n"));
    assert!(text.ends_with("\nEnd rule.\n"));
    assert_eq!(text.matches("<!-- omni-graph:start -->").count(), 1);
    assert_eq!(id(&initial), id(&f.ok(&["project", "show"])));
    assert!(f.root().join(".agents/skills/omni-graph/SKILL.md").exists());
    assert!(f.root().join(".claude/skills/omni-graph/SKILL.md").exists());
    let nested = f.root().join("packages");
    fs::create_dir(&nested).unwrap();
    let out = f.cmd(&nested, &["init"]).output().unwrap();
    assert!(!out.status.success());
    assert!(!nested.join(".omni-graph").exists());
    assert_eq!(f.ok(&["doctor"])["ok"], true);
}

#[test]
fn memory_roundtrip_and_evidence_validation() {
    let f = Fixture::new();
    let memory = f.memory(json!({"title":"Webhook signatures", "type":"decision", "body":"Verify before parsing", "evidence_level":"verified", "sources":["src/webhooks.rs:42"], "topics":["payments"]}));
    assert!(memory["verified_at"].is_string());
    let shown = f.ok(&["show", id(&memory)]);
    assert_eq!(shown["memory"]["title"], "Webhook signatures");
    assert_eq!(
        f.ok(&["search", "signatures", "--topic", "payments"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        f.fail(&[
            "remember",
            "--title",
            "Bad evidence",
            "--evidence",
            "verified"
        ])
        .contains("source")
    );
    assert!(
        f.fail(&["remember", "--title", "No task", "--scope", "task"])
            .contains("task_id")
    );
    assert!(
        f.fail(&["remember", "--title", "Webhook signatures"])
            .contains("already exists")
    );
    let patch = f.stdin(
        &["update", id(&memory), "--file", "-"],
        json!({"body":"Verify raw request bytes"}),
    );
    assert_eq!(patch["evidence_level"], "inferred");
    assert!(patch["verified_at"].is_null());
    assert_eq!(id(&patch), id(&memory));
}

#[test]
fn client_and_project_boundaries_are_enforced() {
    let f = Fixture::new();
    let client = f.ok(&["client", "add", "Northstar"]);
    let other_client = f.ok(&["client", "add", "Other Co"]);
    let a = f.temp.path().join("a");
    let b = f.temp.path().join("b");
    let c = f.temp.path().join("c");
    for (root, client_id) in [
        (&a, id(&client)),
        (&b, id(&client)),
        (&c, id(&other_client)),
    ] {
        fs::create_dir(root).unwrap();
        parse(
            f.cmd(root, &["init", "--client", client_id, "--agent", "none"])
                .output()
                .unwrap(),
        );
    }
    parse(
        f.cmd(
            &a,
            &[
                "remember",
                "--title",
                "Secret Project A",
                "--body",
                "private knowledge",
            ],
        )
        .output()
        .unwrap(),
    );
    parse(
        f.cmd(
            &a,
            &["remember", "--title", "Client style", "--scope", "client"],
        )
        .output()
        .unwrap(),
    );
    f.ok(&[
        "remember",
        "--title",
        "Use pnpm",
        "--type",
        "preference",
        "--scope",
        "global",
    ]);
    let b_memories = parse(f.cmd(&b, &["list"]).output().unwrap());
    let b_text = b_memories.to_string();
    assert!(b_text.contains("Client style") && b_text.contains("Use pnpm"));
    assert!(!b_text.contains("Secret Project A"));
    let c_text = parse(f.cmd(&c, &["list"]).output().unwrap()).to_string();
    assert!(!c_text.contains("Client style") && !c_text.contains("Secret Project A"));
    assert!(c_text.contains("Use pnpm"));
}

#[test]
fn supersession_is_atomic_and_reactivation_requires_unlinking() {
    let f = Fixture::new();
    let old = f.memory(json!({"title":"Old decision"}));
    let new = f.memory(json!({"title":"New decision"}));
    f.ok(&["link", id(&new), "supersedes", id(&old)]);
    f.ok(&["link", id(&new), "supersedes", id(&old)]);
    assert_eq!(f.ok(&["show", id(&old)])["memory"]["status"], "superseded");
    assert_eq!(f.ok(&["list"]).as_array().unwrap().len(), 1);
    assert_eq!(f.ok(&["list", "--all"]).as_array().unwrap().len(), 2);
    f.ok(&["unlink", id(&new), "supersedes", id(&old)]);
    f.stdin(
        &["update", id(&old), "--file", "-"],
        json!({"status":"active"}),
    );
    assert_eq!(f.ok(&["list"]).as_array().unwrap().len(), 2);
}

#[test]
fn recall_uses_triggers_and_neighbors_but_excludes_unrelated_tasks_and_expired_records() {
    let f = Fixture::new();
    let lesson = f.memory(json!({"title":"Inspect consumers", "type":"lesson", "triggers":["changing shared props"], "prevention":["Find all usages"]}));
    let fix = f.memory(json!({"title":"Consumer repair", "type":"fix"}));
    f.ok(&["link", id(&lesson), "related_to", id(&fix)]);
    f.memory(json!({"title":"Changing shared props investigation", "type":"handoff", "scope":"task", "task_id":"other-task"}));
    f.memory(json!({"title":"Changing shared props expired", "expires_at":"2000-01-01T00:00:00Z"}));
    let recall = f.ok(&["recall", "shared props"]);
    let hits = recall["results"].as_array().unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().any(
        |h| h["memory"]["id"] == id(&fix) && h["reason"].as_str().unwrap().contains("neighbor")
    ));
    assert!(!recall.to_string().contains("other-task"));
    assert_eq!(
        f.ok(&["recall", "shared", "--limit", "1"])["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.ok(&["recall", "shared", "--task", "other-task"])["results"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        f.ok(&["graph", "--root", id(&lesson), "--depth", "0"])["memories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn learning_loop_records_failed_attempts_verified_fixes_and_recurring_causes() {
    let f = Fixture::new();
    let mistake = f.memory(json!({"title":"Missed shared consumer", "type":"mistake", "cause":"Only checked one usage"}));
    f.memory(json!({"title":"Missed another consumer", "type":"mistake", "cause":"only checked one usage"}));
    let fix = f.memory(json!({"title":"Update consumers", "type":"fix", "body":"First attempt failed. Updating both usages resolved the issue."}));
    let lesson = f.memory(json!({"title":"Inspect all consumers", "type":"lesson", "triggers":["Changing props"], "prevention":["Find all usages"]}));
    f.ok(&["link", id(&mistake), "corrected_by", id(&fix)]);
    f.ok(&["link", id(&mistake), "teaches", id(&lesson)]);
    let fixed = f.stdin(&["update", id(&fix), "--file", "-"], json!({"progress":"verified", "evidence_level":"verified", "verification":["Consumer type check passed"], "sources":["CI run 42"]}));
    assert!(fixed["verified_at"].is_string());
    let review = f.ok(&["review"]);
    assert_eq!(review["repeated_causes"].as_array().unwrap().len(), 1);
    assert!(
        !review["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == id(&fix))
    );
    assert!(
        review["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == id(&mistake))
    );
    assert_eq!(f.ok(&["doctor"])["ok"], true);
}

#[test]
fn promotion_requires_confirmation_and_generalized_content() {
    let f = Fixture::new();
    let memory = f.memory(json!({"title":"Secret Client Finding", "type":"lesson", "body":"Private customer detail", "keywords":["secret-name"], "sources":["private-url"]}));
    assert!(
        f.fail(&[
            "promote",
            id(&memory),
            "--title",
            "Generic lesson",
            "--body",
            "General takeaway"
        ])
        .contains("authorization")
    );
    let promoted = f.ok(&[
        "promote",
        id(&memory),
        "--title",
        "Generic lesson",
        "--body",
        "General takeaway",
        "--confirm",
    ]);
    assert_eq!(promoted["scope"], "global");
    assert!(promoted["project_id"].is_null());
    assert!(!promoted.to_string().contains("private-url"));
    assert!(!promoted.to_string().contains("secret-name"));
    assert!(
        f.fail(&["link", id(&promoted), "related_to", id(&memory)])
            .contains("broader scope")
    );
    f.ok(&["link", id(&memory), "derived_from", id(&promoted)]);
}

#[test]
fn concurrent_writers_do_not_lose_memories() {
    let f = Fixture::new();
    let children: Vec<_> = (0..16)
        .map(|i| {
            f.cmd(&f.root(), &["remember", "--title", &format!("Memory {i}")])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(f.ok(&["list"]).as_array().unwrap().len(), 16);
}

#[test]
fn corrupt_store_is_reported_and_never_overwritten() {
    let f = Fixture::new();
    let path = f.root().join(".omni-graph/graph.json");
    fs::remove_file(f.root().join(".omni-graph/memory.sqlite")).unwrap();
    fs::write(&path, "{broken").unwrap();
    assert!(
        f.fail(&["remember", "--title", "Cannot save"])
            .contains("invalid JSON")
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "{broken");
}

#[test]
fn skill_setup_rejects_malformed_markers_before_overwriting_other_files() {
    let f = Fixture::new();
    fs::write(f.root().join("CLAUDE.md"), "<!-- omni-graph:start -->").unwrap();
    let before = fs::read(f.root().join("AGENTS.md")).unwrap();
    assert!(f.fail(&["setup"]).contains("markers"));
    assert_eq!(before, fs::read(f.root().join("AGENTS.md")).unwrap());
}

#[cfg(unix)]
#[test]
fn skill_setup_does_not_follow_symlinks() {
    let f = Fixture::new();
    let elsewhere = f.temp.path().join("outside.md");
    fs::write(&elsewhere, "untouched").unwrap();
    fs::remove_file(f.root().join("CLAUDE.md")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, f.root().join("CLAUDE.md")).unwrap();
    assert!(f.fail(&["setup"]).contains("symlink"));
    assert_eq!(fs::read_to_string(elsewhere).unwrap(), "untouched");
}

#[test]
fn task_recall_preserves_global_preferences_and_project_knowledge() {
    let f = Fixture::new();
    f.memory(json!({"title":"Use pnpm", "type":"preference", "scope":"global"}));
    f.memory(json!({"title":"Authentication architecture", "body":"Cookie sessions"}));
    f.memory(json!({"title":"Authentication handoff", "type":"handoff", "scope":"task", "task_id":"task-42"}));
    let recall = f.ok(&["recall", "authentication", "--task", "task-42"]);
    let titles: Vec<_> = recall["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["memory"]["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"Use pnpm"));
    assert!(titles.contains(&"Authentication architecture"));
    assert!(titles.contains(&"Authentication handoff"));
}

#[test]
fn export_excludes_private_knowledge_and_import_is_project_bound_and_idempotent() {
    let f = Fixture::new();
    f.memory(json!({"title":"Private secret"}));
    f.memory(json!({"title":"Global preference", "scope":"global"}));
    f.memory(json!({"title":"Team lesson", "visibility":"project_shared", "type":"lesson"}));
    let exported = f.temp.path().join("shared.json");
    let result = f.ok(&["export", "--output", exported.to_str().unwrap()]);
    assert_eq!(result["memories"], 1);
    let text = fs::read_to_string(&exported).unwrap();
    assert!(!text.contains("Private secret") && !text.contains("Global preference"));
    assert_eq!(
        f.ok(&["import", "--file", exported.to_str().unwrap()])["imported"],
        0
    );
    // A second checkout with the same identity can import the exported knowledge.
    let clone = f.temp.path().join("clone");
    fs::create_dir_all(clone.join(".omni-graph")).unwrap();
    fs::copy(
        f.root().join(".omni-graph/project.json"),
        clone.join(".omni-graph/project.json"),
    )
    .unwrap();
    let imported = parse(
        f.cmd(&clone, &["import", "--file", exported.to_str().unwrap()])
            .output()
            .unwrap(),
    );
    assert_eq!(imported["imported"], 1);
    let memories = parse(
        f.cmd(&clone, &["list", "--scope", "project"])
            .output()
            .unwrap(),
    );
    assert_eq!(memories.as_array().unwrap().len(), 1);
    // A different project must not be able to import the bundle.
    let other = f.temp.path().join("other");
    fs::create_dir(&other).unwrap();
    parse(
        f.cmd(&other, &["init", "--agent", "none"])
            .output()
            .unwrap(),
    );
    assert!(
        !f.cmd(&other, &["import", "--file", exported.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn import_rejects_supersession_cycles_without_changing_the_store() {
    let f = Fixture::new();
    let a = f.memory(json!({"title":"First shared decision", "visibility":"project_shared"}));
    let b = f.memory(json!({"title":"Second shared decision", "visibility":"project_shared"}));
    let exported = f.temp.path().join("cycle.json");
    f.ok(&["export", "--output", exported.to_str().unwrap()]);
    let mut bundle: Value = serde_json::from_slice(&fs::read(&exported).unwrap()).unwrap();
    for memory in bundle["graph"]["memories"].as_array_mut().unwrap() {
        memory["status"] = json!("superseded");
    }
    bundle["graph"]["edges"] = json!([
        {"from":id(&a),"to":id(&b),"relation":"supersedes"},
        {"from":id(&b),"to":id(&a),"relation":"supersedes"}
    ]);
    fs::write(&exported, bundle.to_string()).unwrap();
    assert!(
        f.fail(&["import", "--file", exported.to_str().unwrap()])
            .contains("cycle")
    );
    assert_eq!(f.ok(&["list"]).as_array().unwrap().len(), 2);
}

#[test]
fn sqlite_storage_is_initialized_and_private_files_stay_ignored() {
    let f = Fixture::new();
    let path = f.root().join(".omni-graph/memory.sqlite");
    assert_eq!(&fs::read(&path).unwrap()[..16], b"SQLite format 3\0");
    assert!(!f.root().join(".omni-graph/graph.json").exists());
    let ignore = f.root().join(".omni-graph/.gitignore");
    fs::write(&ignore, "# Keep my rule\n/custom-private-file\n").unwrap();
    f.ok(&["list"]);
    let text = fs::read_to_string(&ignore).unwrap();
    assert!(text.starts_with("# Keep my rule\n/custom-private-file\n"));
    for pattern in [
        "/memory.sqlite",
        "/memory.sqlite-wal",
        "/memory.sqlite-shm",
        "/memory.sqlite-journal",
    ] {
        assert!(text.lines().any(|line| line == pattern));
    }
    let doctor = f.ok(&["doctor"]);
    assert_eq!(doctor["ok"], true);
    assert_eq!(doctor["storage"]["search"], "fts5");
}

#[test]
fn legacy_json_migrates_once_with_ids_edges_and_a_searchable_index() {
    let f = Fixture::new();
    let lesson = f.memory(json!({"title":"Inspect consumers", "type":"lesson", "triggers":["Changing shared properties"]}));
    let mistake = f.memory(json!({"title":"Missed consumer", "type":"mistake"}));
    f.ok(&["link", id(&mistake), "teaches", id(&lesson)]);
    let graph = f.ok(&["graph", "--all"]);
    let directory = f.root().join(".omni-graph");
    fs::remove_file(directory.join("memory.sqlite")).unwrap();
    let legacy = serde_json::to_string_pretty(&graph).unwrap();
    fs::write(directory.join("graph.json"), &legacy).unwrap();
    let results = f.ok(&["search", "properties"]);
    assert_eq!(results.as_array().unwrap().len(), 1);
    assert_eq!(results[0]["memory"]["id"], id(&lesson));
    assert_eq!(f.ok(&["graph", "--all"]), graph);
    assert_eq!(
        fs::read_to_string(directory.join("graph.json")).unwrap(),
        legacy
    );
    f.stdin(
        &["update", id(&lesson), "--file", "-"],
        json!({"title":"Inspect every consumer"}),
    );
    assert_eq!(
        f.ok(&["show", id(&lesson)])["memory"]["title"],
        "Inspect every consumer"
    );
    assert_eq!(
        fs::read_to_string(directory.join("graph.json")).unwrap(),
        legacy
    );
    assert_eq!(f.ok(&["doctor"])["ok"], true);
}

#[test]
fn fts_handles_stemming_diacritics_prefixes_and_literal_user_input() {
    let f = Fixture::new();
    let memory = f.memory(json!({"title":"Café deployment notes", "body":"Consumers running migrations safely", "keywords":["authentication"], "cause":"Uncommitted transactions"}));
    for query in ["cafe", "consumer", "auth", "transaction"] {
        let results = f.ok(&["search", query]);
        assert_eq!(results.as_array().unwrap().len(), 1, "{query}");
        assert_eq!(results[0]["memory"]["id"], id(&memory));
        assert!(results[0]["text_rank"].is_number());
    }
    assert!(
        f.ok(&["search", "\" * : () -"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.ok(&["search", "' ; DROP TABLE imaginary; --"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.ok(&["doctor"])["ok"], true);
}

#[test]
fn fts_updates_remove_old_terms_and_reindex_restores_missing_entries() {
    let f = Fixture::new();
    let memory = f.memory(json!({"title":"Zebras grazing", "body":"Savannah animals"}));
    assert_eq!(f.ok(&["search", "zebra"]).as_array().unwrap().len(), 1);
    f.stdin(
        &["update", id(&memory), "--file", "-"],
        json!({"title":"Dolphins swimming", "body":"Ocean mammals"}),
    );
    assert!(f.ok(&["search", "zebra"]).as_array().unwrap().is_empty());
    assert!(f.ok(&["search", "savannah"]).as_array().unwrap().is_empty());
    assert_eq!(f.ok(&["search", "dolphin"]).as_array().unwrap().len(), 1);
    // Deliberately damage only the derived index; authoritative memory survives.
    let connection =
        rusqlite::Connection::open(f.root().join(".omni-graph/memory.sqlite")).unwrap();
    connection
        .execute(
            "INSERT INTO memory_fts(memory_fts) VALUES('delete-all')",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(f.ok(&["search", "dolphin"]).as_array().unwrap().is_empty());
    assert_eq!(f.ok(&["doctor"])["ok"], false);
    assert_eq!(f.ok(&["reindex"])["rebuilt"], true);
    assert_eq!(f.ok(&["search", "dolphin"]).as_array().unwrap().len(), 1);
    assert_eq!(f.ok(&["doctor"])["ok"], true);
}

#[test]
fn failed_sql_write_rolls_back_memory_edges_and_full_text_changes() {
    let f = Fixture::new();
    let old = f.memory(json!({"title":"Old deployment decision"}));
    let new = f.memory(json!({"title":"New deployment decision"}));
    let connection =
        rusqlite::Connection::open(f.root().join(".omni-graph/memory.sqlite")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_test_edge BEFORE INSERT ON edges BEGIN SELECT RAISE(ABORT, 'test edge failure'); END;").unwrap();
    drop(connection);
    assert!(
        f.fail(&["link", id(&new), "supersedes", id(&old)])
            .contains("test edge failure")
    );
    assert_eq!(f.ok(&["show", id(&old)])["memory"]["status"], "active");
    assert!(
        f.ok(&["show", id(&new)])["edges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.ok(&["search", "deployment"]).as_array().unwrap().len(), 2);
    assert_eq!(f.ok(&["doctor"])["ok"], true);
}

#[test]
fn corrupt_sqlite_is_not_replaced_with_legacy_data() {
    let f = Fixture::new();
    let directory = f.root().join(".omni-graph");
    fs::write(directory.join("memory.sqlite"), "not a database").unwrap();
    fs::write(
        directory.join("graph.json"),
        r#"{"schema_version":1,"memories":[],"edges":[]}"#,
    )
    .unwrap();
    assert!(
        f.fail(&["remember", "--title", "Cannot save"])
            .contains("not a database")
    );
    assert_eq!(
        fs::read_to_string(directory.join("memory.sqlite")).unwrap(),
        "not a database"
    );
}

#[test]
fn full_text_search_respects_client_and_project_boundaries() {
    let f = Fixture::new();
    let client = f.ok(&["client", "add", "Northstar"]);
    let a = f.temp.path().join("fts-a");
    let b = f.temp.path().join("fts-b");
    for root in [&a, &b] {
        fs::create_dir(root).unwrap();
        parse(
            f.cmd(root, &["init", "--client", id(&client), "--agent", "none"])
                .output()
                .unwrap(),
        );
    }
    parse(
        f.cmd(
            &a,
            &[
                "remember",
                "--title",
                "Secret deployment",
                "--scope",
                "project",
            ],
        )
        .output()
        .unwrap(),
    );
    parse(
        f.cmd(
            &a,
            &[
                "remember",
                "--title",
                "Client deployment",
                "--scope",
                "client",
            ],
        )
        .output()
        .unwrap(),
    );
    f.ok(&[
        "remember",
        "--title",
        "Global deployment",
        "--scope",
        "global",
    ]);
    let results = parse(f.cmd(&b, &["search", "deployment"]).output().unwrap());
    assert_eq!(results.as_array().unwrap().len(), 2);
    assert!(!results.to_string().contains("Secret deployment"));
    let outside = f.ok(&["search", "deployment"]);
    assert_eq!(outside.as_array().unwrap().len(), 1);
    assert!(!outside.to_string().contains("Client deployment"));
}

#[test]
fn install_initializes_the_default_user_home_without_touching_the_project() {
    let temp = tempfile::tempdir().unwrap();
    let user = temp.path().join("user");
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let result = parse(
        Command::new(env!("CARGO_BIN_EXE_omni-graph"))
            .env_remove("OMNI_GRAPH_HOME")
            .env("HOME", &user)
            .env("USERPROFILE", &user)
            .env("SHELL", "/bin/bash")
            .current_dir(&project)
            .args(["install", "--json"])
            .output()
            .unwrap(),
    );
    let home = user.join(".omni-graph");
    assert_eq!(result["home"], home.to_string_lossy().as_ref());
    assert!(home.join("memory.sqlite").is_file());
    assert!(home.join("registry.json").is_file());
    assert!(!project.join(".omni-graph").exists());
    assert!(!project.join("AGENTS.md").exists());
    let binary = result["binary"].as_str().unwrap();
    let version = Command::new(binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    // Running install from its own destination must not overwrite a running executable.
    let again = parse(
        Command::new(binary)
            .env("OMNI_GRAPH_HOME", &home)
            .env("HOME", &user)
            .env("USERPROFILE", &user)
            .env("SHELL", "/bin/bash")
            .args(["install", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(again["binary"], result["binary"]);
}

#[test]
fn reinstall_preserves_registered_projects_and_global_memories() {
    let f = Fixture::new();
    let memory =
        f.memory(json!({"title":"Use clear language", "scope":"global", "type":"preference"}));
    let installed = f.ok(&["install"]);
    f.ok(&["install"]);
    assert_eq!(
        f.ok(&["show", memory["id"].as_str().unwrap()])["memory"]["title"],
        "Use clear language"
    );
    let registry: Value =
        serde_json::from_slice(&fs::read(f.temp.path().join("home/registry.json")).unwrap())
            .unwrap();
    assert_eq!(registry["projects"].as_array().unwrap().len(), 1);
    assert!(Path::new(installed["binary"].as_str().unwrap()).is_file());
}

#[cfg(unix)]
#[test]
fn install_configures_shell_profiles_and_respects_login_profile_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let user = temp.path().join("user");
    fs::create_dir_all(&user).unwrap();
    fs::write(user.join(".bash_profile"), "# my login config\n").unwrap();
    fs::write(user.join(".profile"), "# not the active bash login file\n").unwrap();
    let home = temp.path().join("custom memory ' quoted");
    let install = || {
        parse(
            Command::new(env!("CARGO_BIN_EXE_omni-graph"))
                .env("HOME", &user)
                .env("SHELL", "/bin/bash")
                .env("OMNI_GRAPH_HOME", &home)
                .args(["install", "--json"])
                .output()
                .unwrap(),
        )
    };
    let first = install();
    assert_eq!(first["path_modified"], true);
    assert_eq!(first["path_profiles"].as_array().unwrap().len(), 2);
    assert!(
        fs::read_to_string(user.join(".bash_profile"))
            .unwrap()
            .starts_with("# my login config\n")
    );
    assert_eq!(
        fs::read_to_string(user.join(".profile")).unwrap(),
        "# not the active bash login file\n"
    );
    assert_eq!(install()["path_modified"], false);
    let command = format!(
        "{}; command -v omni-graph",
        first["activate"].as_str().unwrap()
    );
    let current = Command::new("/bin/bash")
        .env("HOME", &user)
        .env("PATH", "/usr/bin:/bin")
        .args(["--noprofile", "--norc", "-c", &command])
        .output()
        .unwrap();
    assert!(
        current.status.success(),
        "{}",
        String::from_utf8_lossy(&current.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        home.join("bin/omni-graph").to_string_lossy()
    );
}

#[cfg(unix)]
#[test]
fn install_reports_profile_errors_without_overwriting_user_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let user = temp.path().join("user");
    fs::create_dir_all(&user).unwrap();
    let damaged = "# personal\n# >>> omni-graph PATH >>>\n";
    fs::write(user.join(".bashrc"), damaged).unwrap();
    let result = parse(
        Command::new(env!("CARGO_BIN_EXE_omni-graph"))
            .env("HOME", &user)
            .env("SHELL", "/bin/bash")
            .env("OMNI_GRAPH_HOME", temp.path().join("memory"))
            .args(["install", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(user.join(".bashrc")).unwrap(), damaged);
    assert!(Path::new(result["binary"].as_str().unwrap()).is_file());
    assert!(
        result["activate"]
            .as_str()
            .unwrap()
            .starts_with("export PATH=")
    );
}

struct WebProcess(std::process::Child);
impl Drop for WebProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn web_request(address: &str, method: &str, path: &str, headers: &str) -> String {
    use std::{io::Read, net::TcpStream, time::Duration};
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n{headers}\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
#[test]
fn browser_server_serves_scoped_live_memory_and_rejects_unsafe_requests() {
    use std::io::{BufRead, BufReader};
    let f = Fixture::new();
    let saved =
        f.memory(json!({"title":"Web visible memory", "body":"<script>literal text</script>"}));
    let global = f.memory(json!({"title":"Shared global preference", "scope":"global"}));
    let foreign = f.temp.path().join("other-project");
    fs::create_dir(&foreign).unwrap();
    parse(
        f.cmd(&foreign, &["init", "--name", "Other project"])
            .output()
            .unwrap(),
    );
    parse(
        f.cmd(
            &foreign,
            &["remember", "--title", "Unrelated project secret"],
        )
        .output()
        .unwrap(),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_omni-graph"))
        .env("OMNI_GRAPH_HOME", f.temp.path().join("home"))
        .args([
            "--directory",
            f.root().to_str().unwrap(),
            "web",
            "--port",
            "0",
            "--no-open",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut server = WebProcess(child);
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let address = line
        .trim()
        .strip_prefix("Omni-graph Web UI: http://")
        .expect("server URL");
    let response = web_request(address, "GET", "/", "");
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("Memory explorer") && response.contains("Content-Security-Policy:"));
    for path in [
        "/app.js",
        "/graph.js",
        "/app.css",
        "/view.js",
        "/types.js",
        "/vendor/three.module.js",
        "/vendor/three.core.js",
        "/vendor/OrbitControls.js",
        "/vendor/THREE-LICENSE.txt",
    ] {
        assert!(web_request(address, "GET", path, "").starts_with("HTTP/1.1 200"));
    }
    let snapshot = || {
        let response = web_request(address, "GET", "/api/graph", "");
        assert!(response.starts_with("HTTP/1.1 200"));
        serde_json::from_str::<Value>(response.split("\r\n\r\n").nth(1).unwrap()).unwrap()
    };
    let initial = snapshot();
    let memories = initial["graph"]["memories"].as_array().unwrap();
    assert_eq!(memories.len(), 2);
    assert!(memories.iter().any(|m| m["id"] == saved["id"]));
    assert!(memories.iter().any(|m| m["id"] == global["id"]));
    let patch = f.temp.path().join("web-update.json");
    fs::write(
        &patch,
        r#"{"body":"Agent changed this while the browser was open"}"#,
    )
    .unwrap();
    f.ok(&["update", id(&saved), "--file", patch.to_str().unwrap()]);
    assert!(
        snapshot()["graph"]["memories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"].as_str().unwrap().contains("Agent changed"))
    );
    assert!(
        web_request(address, "POST", "/api/graph", "Content-Length: 0\r\n")
            .starts_with("HTTP/1.1 405")
    );
    assert!(
        web_request(
            address,
            "GET",
            "/api/graph",
            "Origin: https://example.com\r\n"
        )
        .starts_with("HTTP/1.1 403")
    );
    assert!(
        web_request(
            address,
            "GET",
            "/api/graph",
            "Sec-Fetch-Site: cross-site\r\n"
        )
        .starts_with("HTTP/1.1 403")
    );
    assert!(web_request(address, "GET", "/../../Cargo.toml", "").starts_with("HTTP/1.1 404"));
    assert!(server.0.try_wait().unwrap().is_none());
}

#[test]
fn remote_access_rejects_missing_or_invalid_email_before_starting_services() {
    let f = Fixture::new();
    for emails in [
        vec![],
        vec!["--email", "*@example.com"],
        vec!["--email", "not-an-email"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_omni-graph"))
            .env("OMNI_GRAPH_HOME", f.temp.path().join("home"))
            .args([
                "--directory",
                f.root().to_str().unwrap(),
                "web",
                "--remote",
                "--port",
                "0",
            ])
            .args(emails)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            output.stdout.is_empty(),
            "must not start a listener/helper before validation"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("email"));
        assert!(!f.temp.path().join("home/tools").exists());
    }
}
