//! Selective CLI retrieval. Full graph snapshots remain available for graph/UI work.
//! Ranking and eligibility stay in retrieval.rs; SQLite fetches only text candidates
//! and the bounded neighborhood needed for the requested result window.
use crate::{
    cli::Filter,
    database,
    model::*,
    retrieval::{self, Hit},
    storage::Workspace,
};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet, VecDeque};

pub fn search(
    workspace: &Workspace,
    query: &str,
    filter: &Filter,
    recall: bool,
    depth: u8,
) -> Result<(Vec<Hit>, Vec<Edge>)> {
    let mut connections = workspace
        .stores()?
        .into_iter()
        .map(|store| store.connection())
        .collect::<Result<Vec<_>>>()?;
    // Keep each database's read snapshot throughout seed/neighbor selection.
    // As with Workspace::snapshot, independent stores are not one atomic snapshot.
    let transactions = connections
        .iter_mut()
        .map(Connection::transaction)
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let readers: Vec<&Connection> = transactions.iter().map(|tx| &**tx).collect();
    let result = select(workspace, &readers, query, filter, recall, depth)?;
    for transaction in transactions {
        transaction.commit()?;
    }
    Ok(result)
}

fn select(
    workspace: &Workspace,
    readers: &[&Connection],
    query: &str,
    filter: &Filter,
    recall: bool,
    depth: u8,
) -> Result<(Vec<Hit>, Vec<Edge>)> {
    let mut graph = Graph {
        schema_version: 1,
        ..Graph::default()
    };
    let mut ranks = HashMap::new();
    for reader in readers {
        let matches = database::matches(reader, query)?;
        let ids = serde_json::to_string(&matches.keys().collect::<Vec<_>>())?;
        let mut statement = reader.prepare_cached(if query.trim().is_empty() {
            "SELECT record FROM memories ORDER BY rowid"
        } else if recall {
            "SELECT record FROM memories WHERE id IN (SELECT value FROM json_each(?1)) OR kind IN ('preference','constraint') ORDER BY rowid"
        } else {
            "SELECT record FROM memories WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY rowid"
        })?;
        let mut rows = if query.trim().is_empty() {
            statement.query([])?
        } else {
            statement.query([ids])?
        };
        while let Some(row) = rows.next()? {
            let memory: Memory = serde_json::from_str(&row.get::<_, String>(0)?)?;
            if workspace.visible(&memory) {
                graph.memories.push(memory);
            }
        }
        ranks.extend(matches);
    }
    let ranked = retrieval::search(&graph, query, filter, recall, 0, &ranks);
    if !recall || depth == 0 {
        let edges = selected_edges(readers, &ranked)?;
        return Ok((ranked, edges));
    }
    let limit = filter.limit as usize;
    let mut hits: Vec<_> = ranked.iter().take(limit.div_ceil(2)).cloned().collect();
    let mut seen: HashSet<_> = hits.iter().map(|hit| hit.memory.id.clone()).collect();
    let mut queue: VecDeque<_> = hits.iter().map(|hit| (hit.memory.id.clone(), 0)).collect();
    let mut cache: HashMap<String, Option<Memory>> = HashMap::new();
    for memory in graph.memories {
        cache.entry(memory.id.clone()).or_insert(Some(memory));
    }
    while let Some((node, distance)) = queue.pop_front() {
        if distance >= depth || hits.len() >= limit {
            continue;
        }
        // Store order and row order preserve the existing graph expansion behavior.
        for reader in readers {
            let mut statement = reader.prepare_cached(
                "SELECT source,relation,target FROM edges WHERE source=?1 OR target=?1 ORDER BY rowid"
            )?;
            let mut rows = statement.query([&node])?;
            while let Some(row) = rows.next()? {
                let edge = edge(row)?;
                let neighbor = if edge.from == node {
                    &edge.to
                } else {
                    &edge.from
                };
                if seen.contains(neighbor) {
                    continue;
                }
                if !cache.contains_key(neighbor) {
                    cache.insert(
                        neighbor.clone(),
                        find_visible(workspace, readers, neighbor)?,
                    );
                }
                let Some(memory) = cache.get(neighbor).and_then(Option::as_ref) else {
                    continue;
                };
                if !retrieval::eligible(memory, filter, recall) {
                    continue;
                }
                seen.insert(neighbor.clone());
                hits.push(Hit {
                    memory: memory.clone(),
                    score: 0,
                    reason: format!("graph neighbor via {}", label(&edge.relation)),
                    text_rank: ranks.get(neighbor).copied(),
                });
                queue.push_back((neighbor.clone(), distance + 1));
                if hits.len() >= limit {
                    break;
                }
            }
            if hits.len() >= limit {
                break;
            }
        }
    }
    for hit in ranked {
        if hits.len() >= limit {
            break;
        }
        if seen.insert(hit.memory.id.clone()) {
            hits.push(hit);
        }
    }
    let edges = selected_edges(readers, &hits)?;
    Ok((hits, edges))
}

fn find_visible(
    workspace: &Workspace,
    readers: &[&Connection],
    id: &str,
) -> Result<Option<Memory>> {
    for reader in readers {
        let record: Option<String> = reader
            .prepare_cached("SELECT record FROM memories WHERE id=?1")?
            .query_row([id], |row| row.get(0))
            .optional()?;
        if let Some(record) = record {
            let memory: Memory = serde_json::from_str(&record)?;
            if workspace.visible(&memory) {
                return Ok(Some(memory));
            }
        }
    }
    Ok(None)
}
fn edge(row: &rusqlite::Row<'_>) -> Result<Edge> {
    Ok(Edge {
        from: row.get(0)?,
        relation: serde_json::from_value(serde_json::Value::String(row.get(1)?))?,
        to: row.get(2)?,
    })
}
fn selected_edges(readers: &[&Connection], hits: &[Hit]) -> Result<Vec<Edge>> {
    let ids = serde_json::to_string(&hits.iter().map(|hit| &hit.memory.id).collect::<Vec<_>>())?;
    let mut edges = vec![];
    for reader in readers {
        let mut statement = reader.prepare_cached("SELECT source,relation,target FROM edges WHERE source IN (SELECT value FROM json_each(?1)) AND target IN (SELECT value FROM json_each(?1)) ORDER BY rowid")?;
        let mut rows = statement.query(params![ids])?;
        while let Some(row) = rows.next()? {
            edges.push(edge(row)?);
        }
    }
    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;
    use chrono::{Duration, TimeZone, Utc};

    fn fixture() -> (tempfile::TempDir, Workspace) {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace {
            root: temp.path().join("project"),
            home: temp.path().join("home"),
            project: Project {
                schema_version: 1,
                id: "project_11111111111111111111111111111111".into(),
                name: "Query fixture".into(),
                client_id: Some("client_11111111111111111111111111111111".into()),
                kind: "test".into(),
                description: String::new(),
                repository: None,
                technologies: vec![],
                status: "active".into(),
            },
        };
        storage::write(
            &workspace.home.join("registry.json"),
            &Registry {
                clients: vec![Client {
                    id: "client_11111111111111111111111111111111".into(),
                    name: "Test".into(),
                }],
                projects: vec![],
            },
        )
        .unwrap();
        let stamp = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        for scope in [Scope::Global, Scope::Client, Scope::Project] {
            workspace
                .store(scope)
                .unwrap()
                .change(|graph| {
                    for i in 0..60 {
                        let record_scope = if scope == Scope::Project && i % 6 == 0 {
                            Scope::Task
                        } else {
                            scope
                        };
                        graph.memories.push(Memory {
                            id: format!("mem_{}_{i}", label(&scope)),
                            project_id: matches!(record_scope, Scope::Project | Scope::Task)
                                .then(|| workspace.project.id.clone()),
                            client_id: (record_scope != Scope::Global)
                                .then(|| "client_11111111111111111111111111111111".into()),
                            created_at: stamp,
                            updated_at: stamp + Duration::seconds(i),
                            verified_at: None,
                            data: MemoryInput {
                                title: if i % 5 == 0 {
                                    format!("Zebra migration {i}")
                                } else {
                                    format!("Unrelated record {i}")
                                },
                                body: if i % 7 == 0 {
                                    "Zebra matching body".into()
                                } else {
                                    "Routine content".into()
                                },
                                scope: record_scope,
                                kind: if i % 11 == 0 {
                                    Kind::Preference
                                } else if i % 13 == 0 {
                                    Kind::Constraint
                                } else {
                                    Kind::Fact
                                },
                                task_id: (record_scope == Scope::Task)
                                    .then(|| if i % 12 == 0 { "task_a" } else { "task_b" }.into()),
                                topics: vec![if i % 2 == 0 { "storage" } else { "ui" }.into()],
                                status: if i % 17 == 0 {
                                    Status::Archived
                                } else {
                                    Status::Active
                                },
                                expires_at: (i % 19 == 0).then_some(stamp),
                                ..MemoryInput::default()
                            },
                        });
                    }
                    // Interleaved edges exercise row-order expansion and nonmatching neighbors.
                    for i in 0..59 {
                        graph.edges.push(Edge {
                            from: format!("mem_{}_{i}", label(&scope)),
                            relation: Relation::RelatedTo,
                            to: format!("mem_{}_{}", label(&scope), i + 1),
                        });
                        if scope == Scope::Project {
                            graph.edges.push(Edge {
                                from: format!("mem_project_{i}"),
                                relation: Relation::AppliesTo,
                                to: format!("mem_global_{}", i + 1),
                            });
                        }
                    }
                    if scope == Scope::Project {
                        let mut private = graph.memories[1].clone();
                        private.id = "mem_foreign".into();
                        private.project_id =
                            Some("project_22222222222222222222222222222222".into());
                        private.data.title = "Zebra foreign secret".into();
                        graph.memories.push(private);
                        graph.edges.push(Edge {
                            from: "mem_project_5".into(),
                            relation: Relation::RelatedTo,
                            to: "mem_foreign".into(),
                        });
                    }
                    Ok(())
                })
                .unwrap();
        }
        (temp, workspace)
    }

    #[test]
    fn selective_retrieval_matches_full_snapshot_ranking_edges_and_scope_filters() {
        let (_temp, workspace) = fixture();
        for query in ["zebra", "unmatched phrase", "the", ""] {
            let (graph, ranks) = workspace.indexed_snapshot(query).unwrap();
            for recall in [false, true] {
                for depth in [0, 1, 3] {
                    for filter in [
                        Filter {
                            limit: 7,
                            ..Filter::default()
                        },
                        Filter {
                            limit: 20,
                            task: Some("task_a".into()),
                            ..Filter::default()
                        },
                        Filter {
                            limit: 12,
                            scope: Some(Scope::Task),
                            ..Filter::default()
                        },
                        Filter {
                            limit: 9,
                            kind: Some(Kind::Fact),
                            topic: Some("storage".into()),
                            all: true,
                            ..Filter::default()
                        },
                        Filter {
                            limit: 8,
                            status: Some(Status::Archived),
                            ..Filter::default()
                        },
                    ] {
                        let expected =
                            retrieval::search(&graph, query, &filter, recall, depth, &ranks);
                        let (actual, edges) =
                            search(&workspace, query, &filter, recall, depth).unwrap();
                        assert_eq!(
                            serde_json::to_value(&actual).unwrap(),
                            serde_json::to_value(&expected).unwrap(),
                            "query={query:?} recall={recall} depth={depth}"
                        );
                        let ids: HashSet<_> = expected.iter().map(|hit| &hit.memory.id).collect();
                        let expected_edges: Vec<_> = graph
                            .edges
                            .iter()
                            .filter(|edge| ids.contains(&edge.from) && ids.contains(&edge.to))
                            .cloned()
                            .collect();
                        assert_eq!(edges, expected_edges);
                        assert!(!actual.iter().any(|hit| hit.memory.id == "mem_foreign"));
                    }
                }
            }
        }
    }

    #[test]
    fn unrelated_invalid_record_is_not_loaded_by_nonempty_queries() {
        let (_temp, workspace) = fixture();
        let mut connection = workspace
            .store(Scope::Project)
            .unwrap()
            .connection()
            .unwrap();
        let tx = database::write_transaction(&mut connection).unwrap();
        // Valid SQLite/JSON but invalid Memory. The selective query should never
        // deserialize this unrelated row; the old full snapshot necessarily does.
        tx.execute("INSERT INTO memories(id,record) VALUES('mem_unrelated_bad','{\"id\":\"mem_unrelated_bad\",\"title\":\"Unrelated payload\",\"type\":\"fact\"}')", []).unwrap();
        tx.commit().unwrap();
        let (hits, _) = search(
            &workspace,
            "zebra",
            &Filter {
                limit: 10,
                ..Filter::default()
            },
            true,
            1,
        )
        .unwrap();
        assert!(!hits.is_empty());
        assert!(workspace.indexed_snapshot("zebra").is_err());
    }
}
