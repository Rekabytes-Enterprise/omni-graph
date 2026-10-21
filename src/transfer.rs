use crate::{
    model::*,
    storage::{self, Workspace},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub schema_version: u32,
    pub project_id: String,
    pub graph: Graph,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<crate::attachments::SharedImage>,
}
fn validate_chains(graph: &Graph) -> Result<()> {
    let mut successors = std::collections::HashMap::new();
    for edge in graph
        .edges
        .iter()
        .filter(|e| e.relation == Relation::Supersedes)
    {
        if let Some(previous) = successors.insert(&edge.to, &edge.from) {
            ensure!(
                previous == &edge.from,
                "superseded memory has multiple successors"
            );
        }
    }
    for memory in &graph.memories {
        if memory.data.status == Status::Superseded {
            ensure!(
                successors.contains_key(&memory.id),
                "superseded memory is missing its successor; include the complete shared chain"
            );
        }
        let mut seen = HashSet::new();
        let mut cursor = &memory.id;
        while let Some(next) = successors.get(cursor) {
            ensure!(seen.insert(cursor), "supersession cycle in bundle");
            cursor = next;
        }
    }
    Ok(())
}
pub fn export(workspace: &Workspace, output: &Path) -> Result<Value> {
    let mut graph = workspace.snapshot()?;
    graph.memories.retain(|m| {
        m.data.visibility == Visibility::ProjectShared
            && matches!(m.data.scope, Scope::Project | Scope::Task)
    });
    let ids: HashSet<_> = graph.memories.iter().map(|m| &m.id).collect();
    graph
        .edges
        .retain(|e| ids.contains(&e.from) && ids.contains(&e.to));
    // Refuse an export that accidentally overwrites the live store or profile.
    let absolute = std::path::absolute(output)?;
    let parent = absolute.parent().unwrap().canonicalize()?;
    let destination = parent.join(absolute.file_name().unwrap());
    for store in workspace.stores()? {
        if let Ok(path) = store.path.canonicalize() {
            ensure!(
                !destination.starts_with(path),
                "export outside the live memory stores"
            );
        }
    }
    ensure!(
        !destination.exists(),
        "export destination exists; choose a new filename"
    );
    let count = graph.memories.len();
    let mut images = vec![];
    for memory in &graph.memories {
        for image in crate::attachments::list(
            &workspace.store(memory.data.scope)?.connection()?,
            &memory.id,
        )? {
            images.push(crate::attachments::SharedImage::from_image(
                crate::attachments::read(workspace, &memory.id, &image.id)?,
            ));
        }
    }
    let bundle = Bundle {
        schema_version: if images.is_empty() { 1 } else { 2 },
        project_id: workspace.project.id.clone(),
        graph,
        images,
    };
    validate_chains(&bundle.graph)?;
    storage::write(&destination, &bundle)?;
    Ok(
        json!({"output": destination, "memories": count, "images":bundle.images.len(), "visibility": "project_shared"}),
    )
}
pub fn import(workspace: &Workspace, bundle: Bundle) -> Result<Value> {
    ensure!(
        (bundle.schema_version == 2 || (bundle.schema_version == 1 && bundle.images.is_empty()))
            && bundle.graph.schema_version == 1,
        "unsupported bundle schema"
    );
    ensure!(
        bundle.project_id == workspace.project.id,
        "bundle belongs to a different project; preserve project.json when cloning"
    );
    let mut ids = HashSet::new();
    for m in &bundle.graph.memories {
        m.data.validate()?;
        ensure!(
            valid_id(&m.id, "mem") && ids.insert(m.id.clone()),
            "invalid or duplicate memory ID"
        );
        ensure!(
            m.project_id.as_ref() == Some(&workspace.project.id)
                && m.client_id == workspace.project.client_id,
            "imported memory has different ownership"
        );
        ensure!(
            matches!(m.data.scope, Scope::Project | Scope::Task)
                && m.data.visibility == Visibility::ProjectShared,
            "only project_shared project/task memories can be imported"
        );
    }
    for edge in &bundle.graph.edges {
        ensure!(
            edge.from != edge.to && ids.contains(&edge.from) && ids.contains(&edge.to),
            "invalid bundle edge"
        );
        if edge.relation == Relation::Supersedes {
            let from = bundle
                .graph
                .memories
                .iter()
                .find(|m| m.id == edge.from)
                .unwrap();
            let to = bundle
                .graph
                .memories
                .iter()
                .find(|m| m.id == edge.to)
                .unwrap();
            ensure!(
                from.data.scope == to.data.scope
                    && from.data.task_id == to.data.task_id
                    && to.data.status == Status::Superseded,
                "invalid supersession in bundle"
            );
        }
    }
    validate_chains(&bundle.graph)?;
    let mut image_ids = HashSet::new();
    let images = bundle
        .images
        .into_iter()
        .map(|image| {
            ensure!(
                ids.contains(&image.metadata.memory_id)
                    && image_ids.insert(image.metadata.id.clone()),
                "invalid or duplicate image owner/ID in bundle"
            );
            image.prepare()
        })
        .collect::<Result<Vec<_>>>()?;
    workspace
        .store(Scope::Project)?
        .change_with_images(&images, |graph| {
            let mut added = 0;
            for memory in bundle.graph.memories {
                if let Some(existing) = graph.memories.iter().find(|m| m.id == memory.id) {
                    ensure!(
                        serde_json::to_value(existing)? == serde_json::to_value(&memory)?,
                        "conflicting memory {}; reconcile it explicitly before importing",
                        memory.id
                    );
                } else {
                    ensure!(
                        !graph
                            .memories
                            .iter()
                            .any(|m| m.data.status == Status::Active
                                && memory.data.status == Status::Active
                                && m.data.scope == memory.data.scope
                                && m.data.task_id == memory.data.task_id
                                && m.data
                                    .title
                                    .trim()
                                    .eq_ignore_ascii_case(memory.data.title.trim())),
                        "duplicate active title in bundle"
                    );
                    graph.memories.push(memory);
                    added += 1;
                }
            }
            for edge in bundle.graph.edges {
                if !graph.edges.contains(&edge) {
                    graph.edges.push(edge);
                }
            }
            Ok(json!({"imported": added}))
        })
}
