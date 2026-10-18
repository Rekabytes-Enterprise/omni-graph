use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! enums {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
        #[serde(rename_all = "snake_case")]
        #[value(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}
enums!(Scope {
    Global,
    Client,
    Project,
    Task
});
enums!(Kind {
    Preference,
    Fact,
    Decision,
    Constraint,
    Procedure,
    Lesson,
    Issue,
    Handoff,
    Mistake,
    Fix,
    LearningGoal,
    Component,
    Technology
});
enums!(Status {
    Active,
    Draft,
    Superseded,
    Archived
});
enums!(Evidence {
    Verified,
    Reported,
    Inferred
});
enums!(Visibility {
    Private,
    ProjectShared
});
enums!(Progress {
    Open,
    Investigating,
    FixProposed,
    Fixed,
    Verified
});
enums!(Relation {
    BelongsTo,
    AppliesTo,
    DependsOn,
    Supersedes,
    DerivedFrom,
    Contradicts,
    RelatedTo,
    CorrectedBy,
    Teaches,
    ResolvedBy
});

pub fn id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}
pub fn valid_id(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(&format!("{prefix}_"))
        .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn now() -> DateTime<Utc> {
    Utc::now()
}
pub fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub client_id: Option<String>,
    pub kind: String,
    pub description: String,
    pub repository: Option<String>,
    pub technologies: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registration {
    pub id: String,
    pub name: String,
    pub path: String,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    pub clients: Vec<Client>,
    pub projects: Vec<Registration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryInput {
    pub title: String,
    pub body: String,
    pub summary: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub scope: Scope,
    pub task_id: Option<String>,
    pub topics: Vec<String>,
    pub keywords: Vec<String>,
    pub triggers: Vec<String>,
    pub status: Status,
    pub evidence_level: Evidence,
    pub sources: Vec<String>,
    pub created_by: String,
    pub visibility: Visibility,
    pub review_after: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub context: Option<String>,
    pub failure: Option<String>,
    pub cause: Option<String>,
    pub prevention: Vec<String>,
    pub verification: Vec<String>,
    pub progress: Option<Progress>,
}
impl Default for MemoryInput {
    fn default() -> Self {
        Self {
            title: String::new(),
            body: String::new(),
            summary: String::new(),
            kind: Kind::Fact,
            scope: Scope::Project,
            task_id: None,
            topics: vec![],
            keywords: vec![],
            triggers: vec![],
            status: Status::Active,
            evidence_level: Evidence::Inferred,
            sources: vec![],
            created_by: "unknown".into(),
            visibility: Visibility::Private,
            review_after: None,
            expires_at: None,
            context: None,
            failure: None,
            cause: None,
            prevention: vec![],
            verification: vec![],
            progress: None,
        }
    }
}
impl MemoryInput {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.title.trim().is_empty(), "title is required");
        ensure!(
            self.title.chars().count() <= 240,
            "title must be at most 240 characters"
        );
        if self.scope == Scope::Task {
            ensure!(
                self.task_id.as_ref().is_some_and(|s| !s.trim().is_empty()),
                "task scope requires task_id"
            );
        } else {
            ensure!(self.task_id.is_none(), "task_id requires task scope");
        }
        if self.evidence_level == Evidence::Verified {
            ensure!(
                self.sources.iter().any(|s| !s.trim().is_empty()),
                "verified memories require a source"
            );
        }
        if self.progress == Some(Progress::Verified) {
            ensure!(
                self.verification.iter().any(|s| !s.trim().is_empty())
                    && self.evidence_level == Evidence::Verified,
                "verified progress requires verification details and verified evidence"
            );
        }
        if self.visibility == Visibility::ProjectShared
            && matches!(self.scope, Scope::Global | Scope::Client)
        {
            bail!("project_shared visibility requires project or task scope");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub project_id: Option<String>,
    pub client_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub data: MemoryInput,
}
impl Memory {
    pub fn active(&self) -> bool {
        self.data.status == Status::Active && self.data.expires_at.is_none_or(|t| t > now())
    }
    pub fn due(&self) -> bool {
        self.data.review_after.is_some_and(|t| t <= now())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub relation: Relation,
    pub to: String,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Graph {
    pub schema_version: u32,
    pub memories: Vec<Memory>,
    pub edges: Vec<Edge>,
}
