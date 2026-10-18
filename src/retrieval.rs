use crate::{cli::Filter, model::*};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub memory: Memory,
    pub score: i64,
    pub reason: String,
    pub text_rank: Option<f64>,
}
pub fn search(
    graph: &Graph,
    query: &str,
    filter: &Filter,
    recall: bool,
    depth: u8,
    ranks: &HashMap<String, f64>,
) -> Vec<Hit> {
    let terms: Vec<_> = query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|t| {
            !matches!(
                t.as_str(),
                "the" | "a" | "an" | "and" | "or" | "to" | "of" | "in" | "for"
            )
        })
        .collect();
    let mut context_filter = filter.clone();
    if recall {
        context_filter.task = None;
    }
    let eligible = |m: &Memory| {
        context_filter.matches(m)
            && (!recall
                || m.data.scope != Scope::Task
                || filter
                    .task
                    .as_ref()
                    .is_some_and(|task| m.data.task_id.as_ref() == Some(task))
                || (filter.task.is_none() && filter.scope == Some(Scope::Task)))
    };
    let mut hits = vec![];
    for memory in graph.memories.iter().filter(|m| eligible(m)) {
        let title = memory.data.title.to_lowercase();
        let signals = format!(
            "{} {} {}",
            memory.data.keywords.join(" "),
            memory.data.triggers.join(" "),
            memory.data.topics.join(" ")
        )
        .to_lowercase();
        let text_rank = ranks.get(&memory.id).copied();
        let matched: Vec<_> = terms
            .iter()
            .filter(|t| title.contains(t.as_str()) || signals.contains(t.as_str()))
            .collect();
        let pinned = recall && matches!(memory.data.kind, Kind::Preference | Kind::Constraint);
        if !query.trim().is_empty() && text_rank.is_none() && !pinned {
            continue;
        }
        let mut score = matched
            .iter()
            .map(|t| {
                10 + if title.contains(t.as_str()) { 12 } else { 0 }
                    + if signals.contains(t.as_str()) { 8 } else { 0 }
            })
            .sum::<i64>();
        score += match memory.data.scope {
            Scope::Task => 6,
            Scope::Project => 5,
            Scope::Client => 3,
            Scope::Global => 1,
        };
        score += match memory.data.evidence_level {
            Evidence::Verified => 4,
            Evidence::Reported => 2,
            Evidence::Inferred => 0,
        };
        if memory.due() {
            score -= 5;
        }
        if pinned {
            score += 10;
        }
        if let Some(rank) = text_rank {
            score += 10 + (20.0 * rank.abs() / (1.0 + rank.abs())) as i64;
        }
        hits.push(Hit {
            memory: memory.clone(),
            score,
            reason: if pinned && text_rank.is_none() {
                "applicable preference or constraint".into()
            } else if query.trim().is_empty() {
                "scoped knowledge".into()
            } else {
                "FTS5 full-text match (BM25), scope and evidence".into()
            },
            text_rank,
        });
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(
                a.text_rank
                    .unwrap_or(f64::INFINITY)
                    .total_cmp(&b.text_rank.unwrap_or(f64::INFINITY)),
            )
            .then(b.memory.updated_at.cmp(&a.memory.updated_at))
            .then(a.memory.id.cmp(&b.memory.id))
    });
    // Reserve a bounded portion for related nodes rather than letting seeds consume every slot.
    let limit = filter.limit as usize;
    let ranked = hits.clone();
    let seed_limit = if recall && depth > 0 {
        limit.div_ceil(2)
    } else {
        limit
    };
    hits.truncate(seed_limit);
    if recall && depth > 0 {
        let mut seen: HashSet<_> = hits.iter().map(|h| h.memory.id.clone()).collect();
        let mut queue: VecDeque<_> = hits.iter().map(|h| (h.memory.id.clone(), 0)).collect();
        while let Some((node, distance)) = queue.pop_front() {
            if distance >= depth || hits.len() >= limit {
                continue;
            }
            for edge in &graph.edges {
                let neighbor = if edge.from == node {
                    &edge.to
                } else if edge.to == node {
                    &edge.from
                } else {
                    continue;
                };
                if seen.contains(neighbor) {
                    continue;
                }
                if let Some(memory) = graph
                    .memories
                    .iter()
                    .find(|m| m.id == *neighbor && eligible(m))
                {
                    seen.insert(neighbor.clone());
                    hits.push(Hit {
                        memory: memory.clone(),
                        score: 0,
                        reason: format!("graph neighbor via {}", label(&edge.relation)),
                        text_rank: ranks.get(&memory.id).copied(),
                    });
                    queue.push_back((neighbor.clone(), distance + 1));
                    if hits.len() >= limit {
                        break;
                    }
                }
            }
        }
    }
    // If graph expansion found few neighbors, fill the remaining budget with
    // ranked matches rather than silently returning only half the requested limit.
    let mut seen: HashSet<_> = hits.iter().map(|hit| hit.memory.id.clone()).collect();
    for hit in ranked {
        if hits.len() >= limit {
            break;
        }
        if seen.insert(hit.memory.id.clone()) {
            hits.push(hit);
        }
    }
    hits
}
pub fn neighborhood(graph: &Graph, root: &str, depth: u8) -> HashSet<String> {
    let mut seen = HashSet::from([root.to_owned()]);
    let mut queue = VecDeque::from([(root.to_owned(), 0)]);
    while let Some((node, distance)) = queue.pop_front() {
        if distance >= depth {
            continue;
        }
        for edge in &graph.edges {
            let next = if edge.from == node {
                &edge.to
            } else if edge.to == node {
                &edge.from
            } else {
                continue;
            };
            if seen.insert(next.clone()) {
                queue.push_back((next.clone(), distance + 1));
            }
        }
    }
    seen
}
