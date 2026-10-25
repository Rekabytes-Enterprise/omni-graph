use crate::{integrations::Agent, model::*};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Shared project memory and learning graph for AI agents"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        help = "Project directory; discovers .omni-graph in ancestors"
    )]
    pub directory: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Emit structured JSON (also works before a subcommand)"
    )]
    pub json: bool,
    /// Opt into the stable integration contract; requires --json
    #[arg(long, global = true, requires = "json")]
    pub api_version: Option<u32>,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    /// Inspect compatibility without changing stores or installing software
    Compatibility {
        #[arg(long, value_enum, conflicts_with = "store")]
        scope: Option<Scope>,
        /// Inspect this store directory directly, including a missing store
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Install the executable and initialize user-level memory in ~/.omni-graph
    Install,
    /// Initialize project memory and install project-local agent skills
    Init(Init),
    /// Install/update agent integrations without replacing existing instructions
    Setup {
        #[arg(long, value_enum, value_delimiter = ',', default_value = "all")]
        agent: Vec<Agent>,
    },
    /// Manage registered clients
    Client {
        #[command(subcommand)]
        command: ClientCommand,
    },
    /// Inspect or configure project identity
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Save a memory with optional atomic links supplied in its JSON object
    Remember(Box<Remember>),
    /// Manage visual references stored with a memory
    Image {
        #[command(subcommand)]
        command: ImageCommand,
    },
    /// List accessible memories
    List(Filter),
    /// Search accessible memories using SQLite FTS5 full-text matching
    Search {
        query: String,
        #[command(flatten)]
        filter: Filter,
    },
    /// Recall relevant knowledge plus bounded graph neighbors before working
    Recall {
        #[arg(default_value = "")]
        query: String,
        #[command(flatten)]
        filter: Filter,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=4))]
        depth: u8,
    },
    /// Show a memory and its relationships
    Show { id: String },
    /// Update the application, or edit a memory with ID and --file
    Update {
        #[arg(
            requires = "file",
            help = "Memory ID to edit; omit to update the application"
        )]
        id: Option<String>,
        #[arg(
            long,
            requires = "id",
            help = "Memory JSON patch file, or - for stdin; requires ID"
        )]
        file: Option<PathBuf>,
    },
    /// Add a directed graph edge; supersedes also retires the old memory
    Link {
        from: String,
        #[arg(value_enum)]
        relation: Relation,
        to: String,
    },
    /// Remove an edge (does not reactivate superseded memories)
    Unlink {
        from: String,
        #[arg(value_enum)]
        relation: Relation,
        to: String,
    },
    /// Inspect the graph, optionally centered on a node
    Graph {
        #[arg(long)]
        root: Option<String>,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(0..=4))]
        depth: u8,
        #[arg(long)]
        all: bool,
    },
    /// Distill a project lesson into global knowledge; requires explicit confirmation
    Promote {
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Find review-due memories, unresolved work, and repeated mistakes
    Review,
    /// Export only memories explicitly marked project_shared
    Export {
        #[arg(long)]
        output: PathBuf,
    },
    /// Import shared memories belonging to this stable project identity
    Import {
        #[arg(long)]
        file: PathBuf,
    },
    /// Validate store consistency and agent installation
    Doctor,
    /// Rebuild full-text indexes from saved memory records
    Reindex,
    /// Open the interactive terminal graph
    Tui,
    /// Open the browser graph at http://127.0.0.1:52716
    Web(Web),
}
#[derive(Args)]
pub struct Web {
    #[arg(long, default_value_t = 52716)]
    pub port: u16,
    #[arg(long, help = "Print the local URL without opening a browser")]
    pub no_open: bool,
    #[arg(
        long,
        help = "Generate a temporary HTTPS tunnel protected by email/PIN sign-in"
    )]
    pub remote: bool,
    #[arg(
        long,
        requires = "remote",
        value_delimiter = ',',
        value_name = "EMAIL",
        help = "Email allowed to sign in; repeat to allow others. Prompts when omitted in an interactive terminal"
    )]
    pub email: Vec<String>,
}
impl Default for Web {
    fn default() -> Self {
        Self {
            port: 52716,
            no_open: false,
            remote: false,
            email: vec![],
        }
    }
}
#[derive(Args)]
pub struct Init {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub client: Option<String>,
    #[arg(long, default_value = "freelance")]
    pub kind: String,
    #[arg(long, default_value = "")]
    pub description: String,
    #[arg(long, value_enum, value_delimiter = ',', default_value = "all")]
    pub agent: Vec<Agent>,
}
#[derive(Subcommand)]
pub enum ClientCommand {
    Add {
        name: String,
        #[arg(
            long,
            help = "Existing stable client ID when setting up another machine"
        )]
        id: Option<String>,
    },
    List,
}
#[derive(Subcommand)]
pub enum ProjectCommand {
    Show,
    List,
    Configure {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        repository: Option<String>,
        #[arg(long, value_delimiter = ',')]
        technologies: Option<Vec<String>>,
        #[arg(long)]
        status: Option<String>,
    },
}
#[derive(Args)]
pub struct Remember {
    /// Stable key for retrying the same creation without duplicate writes
    #[arg(long)]
    pub idempotency_key: Option<String>,
    #[arg(
        long,
        help = "Memory JSON object with optional links/images arrays, or - for stdin"
    )]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub body: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "type", value_enum)]
    pub kind: Option<Kind>,
    #[arg(long, value_enum)]
    pub scope: Option<Scope>,
    #[arg(long)]
    pub task: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub topics: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    pub keywords: Option<Vec<String>>,
    #[arg(long = "trigger")]
    pub triggers: Vec<String>,
    #[arg(long = "source")]
    pub sources: Vec<String>,
    #[arg(long, value_enum)]
    pub evidence: Option<Evidence>,
    #[arg(long)]
    pub by: Option<String>,
}
#[derive(Subcommand)]
pub enum ImageCommand {
    Add {
        id: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = "")]
        caption: String,
    },
    List {
        id: String,
    },
    Export {
        id: String,
        image: String,
        #[arg(long)]
        output: PathBuf,
    },
    Remove {
        id: String,
        image: String,
    },
}
#[derive(Args, Default, Clone)]
pub struct Filter {
    #[arg(long, value_enum)]
    pub scope: Option<Scope>,
    #[arg(long = "type", value_enum)]
    pub kind: Option<Kind>,
    #[arg(long)]
    pub task: Option<String>,
    #[arg(long)]
    pub topic: Option<String>,
    #[arg(long, value_enum)]
    pub status: Option<Status>,
    #[arg(long, help = "Include inactive and expired memories")]
    pub all: bool,
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=500))]
    pub limit: u32,
}
impl Filter {
    pub fn matches(&self, m: &Memory) -> bool {
        (self.all || self.status.is_some() || m.active())
            && self.scope.is_none_or(|s| s == m.data.scope)
            && self.kind.is_none_or(|k| k == m.data.kind)
            && self.status.is_none_or(|s| s == m.data.status)
            && self
                .task
                .as_ref()
                .is_none_or(|t| m.data.task_id.as_ref() == Some(t))
            && self
                .topic
                .as_ref()
                .is_none_or(|t| m.data.topics.iter().any(|v| v.eq_ignore_ascii_case(t)))
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    #[test]
    fn update_distinguishes_application_updates_from_memory_patches() {
        let app = Cli::try_parse_from(["omni-graph", "update"]).unwrap();
        assert!(matches!(
            app.command,
            Some(Command::Update {
                id: None,
                file: None
            })
        ));
        let memory =
            Cli::try_parse_from(["omni-graph", "update", "mem_example", "--file", "-"]).unwrap();
        assert!(matches!(
            memory.command,
            Some(Command::Update {
                id: Some(_),
                file: Some(_)
            })
        ));
        assert!(Cli::try_parse_from(["omni-graph", "update", "mem_example"]).is_err());
        assert!(Cli::try_parse_from(["omni-graph", "update", "--file", "-"]).is_err());
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    #[test]
    fn remote_email_arguments_are_explicit_and_repeatable() {
        let cli = Cli::try_parse_from([
            "omni-graph",
            "web",
            "--remote",
            "--email",
            "one@example.com,two@example.com",
            "--email",
            "three@example.com",
        ])
        .unwrap();
        let Some(Command::Web(args)) = cli.command else {
            panic!("expected web options")
        };
        assert!(args.remote);
        assert_eq!(args.email.len(), 3);
        assert_eq!(args.port, 52716);
        assert!(Cli::try_parse_from(["omni-graph", "web", "--email", "one@example.com"]).is_err());
        assert!(Cli::try_parse_from(["omni-graph", "web", "--remote"]).is_ok());
    }
}
