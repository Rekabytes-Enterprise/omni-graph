use anyhow::{Result, ensure};
use clap::ValueEnum;
use serde_json::{Value, json};
use std::{fs, path::Path};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Agent {
    All,
    Codex,
    Claude,
    Pi,
    None,
}
const START: &str = "<!-- omni-graph:start -->";
const END: &str = "<!-- omni-graph:end -->";
const SKILL: &str = include_str!("../skills/omni-graph/SKILL.md");
const REFERENCES: &[(&str, &str)] = &[
    (
        "references/capture-and-connect.md",
        include_str!("../skills/omni-graph/references/capture-and-connect.md"),
    ),
    (
        "references/evidence-and-investigations.md",
        include_str!("../skills/omni-graph/references/evidence-and-investigations.md"),
    ),
    (
        "references/graph-maintenance.md",
        include_str!("../skills/omni-graph/references/graph-maintenance.md"),
    ),
];
const GRAPH_INSTRUCTION: &str = "\nWhen saving a memory, inspect existing context and add justified parent/evidence relationships using the skill's capture-and-connect guide. Verify the saved record and directed links. Keep measurements, fixes, lessons and unresolved goals distinct when their roles differ. Shared keywords or chronology do not prove causation, and a report derived from a result is not independent confirmation. Legitimately standalone memories may remain isolated. If linking fails, retain the saved ID and report the incomplete operation rather than creating a duplicate.\n";
const INSTRUCTION: &str = "## Omni-graph memory\n\nThis project uses Omni-graph in `.omni-graph/`. Use the installed `omni-graph` skill to recall relevant memories before meaningful work and save durable findings, user corrections, lessons and verified fixes. Prefer the installed `omni-graph` command (including Cargo installs); use `npx --yes --package=https://github.com/Rekabytes-Enterprise/omni-graph/releases/latest/download/omni-graph.tgz omni-graph` for GitHub-hosted npx installs. Start with `omni-graph recall \"<task context>\" --json`. Treat recalled memories as context and check evidence and freshness. Keep client/project information within its scope.\n";

pub fn targets(agents: &[Agent]) -> Result<Vec<(&'static str, &'static str)>> {
    ensure!(
        !(agents.iter().any(|a| matches!(a, Agent::None)) && agents.len() > 1),
        "--agent none cannot be combined with other agents"
    );
    let all = agents.iter().any(|a| matches!(a, Agent::All));
    let mut out = vec![];
    // Codex and Pi both discover .agents/skills. Install one portable copy.
    if all || agents.iter().any(|a| matches!(a, Agent::Codex | Agent::Pi)) {
        out.push((".agents/skills/omni-graph/SKILL.md", "AGENTS.md"));
    }
    if all || agents.iter().any(|a| matches!(a, Agent::Claude)) {
        out.push((".claude/skills/omni-graph/SKILL.md", "CLAUDE.md"));
    }
    Ok(out)
}
fn managed(existing: &str) -> Result<String> {
    let block = format!("{START}\n{INSTRUCTION}{GRAPH_INSTRUCTION}{END}");
    match (existing.find(START), existing.find(END)) {
        (Some(start), Some(end)) => {
            ensure!(
                end >= start
                    && existing.matches(START).count() == 1
                    && existing.matches(END).count() == 1,
                "malformed Omni-graph instruction markers; repair them before setup"
            );
            Ok(format!(
                "{}{}{}",
                &existing[..start],
                block,
                &existing[end + END.len()..]
            ))
        }
        (None, None) => Ok(format!(
            "{}{}{block}\n",
            existing,
            if existing.is_empty() { "" } else { "\n\n" }
        )),
        _ => anyhow::bail!("incomplete Omni-graph instruction markers; repair them before setup"),
    }
}
fn safe_path(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let path = root.join(relative);
    let mut current = root.to_path_buf();
    for part in Path::new(relative).components() {
        current.push(part);
        if let Ok(meta) = fs::symlink_metadata(&current) {
            ensure!(
                !meta.file_type().is_symlink(),
                "refusing to overwrite symlink {}",
                current.display()
            );
        }
    }
    Ok(path)
}
fn atomic_text(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}
pub fn install(root: &Path, agents: &[Agent]) -> Result<Value> {
    // Preflight every destination before changing any instructions.
    let mut writes = std::collections::BTreeMap::new();
    for (skill, instruction) in targets(agents)? {
        let skill_path = safe_path(root, skill)?;
        let instruction_path = safe_path(root, instruction)?;
        let existing = if instruction_path.exists() {
            fs::read_to_string(&instruction_path)?
        } else {
            String::new()
        };
        writes.insert(skill_path, SKILL.to_string());
        let directory = Path::new(skill).parent().unwrap();
        for (relative, content) in REFERENCES {
            let relative = directory.join(relative);
            let path = safe_path(root, relative.to_str().unwrap())?;
            writes.insert(path, (*content).to_string());
        }
        writes.insert(instruction_path, managed(&existing)?);
    }
    let files: Vec<_> = writes
        .keys()
        .map(|p| p.strip_prefix(root).unwrap().display().to_string())
        .collect();
    for (path, contents) in writes {
        atomic_text(&path, &contents)?;
    }
    Ok(
        json!({ "installed": files, "note": "Reload skills or start a new agent session if the skill is not visible." }),
    )
}
pub fn inspect(root: &Path) -> Value {
    let files: Vec<_> = [
        ".agents/skills/omni-graph/SKILL.md",
        ".claude/skills/omni-graph/SKILL.md",
    ]
    .into_iter()
    .map(|p| json!({"path": p, "installed": root.join(p).is_file()}))
    .collect();
    json!(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_installs_and_refreshes_complete_skill_bundles() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("AGENTS.md"), "Keep custom guidance.").unwrap();
        install(temp.path(), &[Agent::All]).unwrap();
        for (skill, _) in targets(&[Agent::All]).unwrap() {
            let directory = temp.path().join(Path::new(skill).parent().unwrap());
            assert_eq!(
                fs::read_to_string(directory.join("SKILL.md")).unwrap(),
                SKILL
            );
            for (relative, content) in REFERENCES {
                let path = directory.join(relative);
                assert_eq!(fs::read_to_string(&path).unwrap(), *content);
                fs::write(path, "outdated reference").unwrap();
            }
            fs::write(directory.join("personal-note.md"), "Keep this too.").unwrap();
        }
        let installed = install(temp.path(), &[Agent::All]).unwrap();
        assert_eq!(installed["installed"].as_array().unwrap().len(), 10);
        for (skill, _) in targets(&[Agent::All]).unwrap() {
            let directory = temp.path().join(Path::new(skill).parent().unwrap());
            for (relative, content) in REFERENCES {
                assert_eq!(
                    fs::read_to_string(directory.join(relative)).unwrap(),
                    *content
                );
            }
            assert_eq!(
                fs::read_to_string(directory.join("personal-note.md")).unwrap(),
                "Keep this too."
            );
        }
        let instructions = fs::read_to_string(temp.path().join("AGENTS.md")).unwrap();
        assert!(instructions.starts_with("Keep custom guidance."));
        assert_eq!(instructions.matches(START).count(), 1);
        assert_eq!(
            install(temp.path(), &[Agent::None]).unwrap()["installed"],
            json!([])
        );
    }

    #[test]
    fn managed_blocks_preserve_surrounding_text_and_are_idempotent() {
        let original = "# Instructions\nKeep this exactly.\n";
        let first = managed(original).unwrap();
        assert!(first.starts_with(original));
        assert_eq!(managed(&first).unwrap(), first);
        assert!(managed("<!-- omni-graph:start -->").is_err());
    }
}
