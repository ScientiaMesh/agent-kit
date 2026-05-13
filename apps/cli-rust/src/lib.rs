use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const BINARY_NAME: &str = "smesh";
pub const DEFAULT_API_URL: &str = "https://portal.preview.scientiamesh.app";
pub const DEFAULT_AUTH_AUDIENCE: &str = "https://api.preview.scientiamesh.app";
pub const DEFAULT_AUTH_DOMAIN: &str = "preview-smesh.ca.auth0.com";
pub const DEFAULT_AUTH_CLIENT_ID: &str = "j2jfu9tV6OcGft0C8UPiy3soszr3149L";
pub const DEFAULT_CONFIG_PATH: &str = "~/.config/scientiamesh/config.json";
const USER_AGENT: &str = concat!("smesh-rs/", env!("CARGO_PKG_VERSION"));
const RETRIEVAL_SCHEMA_VERSION: u8 = 1;
const AGENT_PORTABLE_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Parser)]
#[command(
    name = BINARY_NAME,
    bin_name = BINARY_NAME,
    version,
    about = "ScientiaMesh command line interface.",
    long_about = "ScientiaMesh command line interface for humans, scripts, and agents.",
    propagate_version = true
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "SMESH_API_URL",
        default_value = DEFAULT_API_URL,
        value_name = "URL",
        help = "Base URL for the ScientiaMesh portal or API."
    )]
    pub api_url: String,

    #[arg(
        long,
        global = true,
        env = "SMESH_MESH_ID",
        value_name = "UUID",
        help = "Mesh context for commands that operate inside a mesh."
    )]
    pub mesh_id: Option<String>,

    #[arg(
        long,
        global = true,
        default_value = "default",
        value_name = "NAME",
        help = "Named local profile."
    )]
    pub profile: String,

    #[arg(
        long,
        global = true,
        env = "SMESH_CONFIG",
        default_value = DEFAULT_CONFIG_PATH,
        value_name = "PATH",
        help = "Config file path."
    )]
    pub config: PathBuf,

    #[arg(
        long,
        global = true,
        env = "SMESH_TOKEN",
        hide_env_values = true,
        value_name = "TOKEN",
        help = "Use a bearer token for this invocation without writing it."
    )]
    pub token: Option<String>,

    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "MODE",
        help = "Select human, JSON, or newline-delimited JSON output. Defaults to JSON in agent mode and human in interactive terminals."
    )]
    pub output: Option<OutputMode>,

    #[arg(
        long = "json",
        global = true,
        action = ArgAction::SetTrue,
        conflicts_with = "output",
        help = "Alias for --output json."
    )]
    pub json: bool,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        help = "Disable ANSI styling in human output."
    )]
    pub no_color: bool,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        help = "Suppress non-essential human output."
    )]
    pub quiet: bool,

    #[arg(
        long,
        global = true,
        action = ArgAction::SetTrue,
        help = "Include safe debug context in human errors and traces."
    )]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    pub fn effective_output(&self) -> OutputMode {
        self.effective_output_for(agent_mode_enabled())
    }

    pub fn effective_output_for(&self, agent_mode: bool) -> OutputMode {
        if self.json {
            OutputMode::Json
        } else {
            self.output.unwrap_or(if agent_mode {
                OutputMode::Json
            } else {
                OutputMode::Human
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum OutputMode {
    Human,
    Json,
    Ndjson,
}

impl OutputMode {
    fn from_raw_arg(value: &str) -> Option<Self> {
        match value {
            "human" => Some(Self::Human),
            "json" => Some(Self::Json),
            "ndjson" => Some(Self::Ndjson),
            _ => None,
        }
    }
}

pub fn agent_mode_enabled() -> bool {
    env_flag_enabled("SMESH_AGENT_MODE") || !io::stdout().is_terminal()
}

pub fn output_mode_from_raw_args<I, S>(args: I, agent_mode: bool) -> OutputMode
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = args.into_iter().skip(1);
    while let Some(arg) = args.next() {
        let Some(arg) = arg.as_ref().to_str() else {
            continue;
        };

        if arg == "--json" {
            return OutputMode::Json;
        }

        if let Some(value) = arg.strip_prefix("--output=") {
            if let Some(mode) = OutputMode::from_raw_arg(value) {
                return mode;
            }
        } else if arg == "--output" {
            if let Some(value) = args
                .next()
                .and_then(|value| value.as_ref().to_str().and_then(OutputMode::from_raw_arg))
            {
                return value;
            }
        }
    }

    if agent_mode {
        OutputMode::Json
    } else {
        OutputMode::Human
    }
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Inspect or update local authentication state.")]
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },

    #[command(about = "Bootstrap or save portable agent workspace state.")]
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },

    #[command(about = "Inspect queued or completed jobs.")]
    Jobs {
        #[command(subcommand)]
        command: JobCommands,
    },

    #[command(about = "Inspect capture processing state.")]
    Capture {
        #[command(subcommand)]
        command: CaptureCommands,
    },

    #[command(about = "Search mesh knowledge.")]
    Search(SearchArgs),

    #[command(about = "Ask a question against the active mesh.")]
    Ask(AskArgs),

    #[command(about = "Retrieve topic-linked knowledge and activity.")]
    Topics {
        #[command(subcommand)]
        command: TopicCommands,
    },

    #[command(about = "Manage canonical projects.")]
    Projects {
        #[command(subcommand)]
        command: ProjectCommands,
    },

    #[command(about = "Review inferred canonical record assertions.")]
    Assertions {
        #[command(subcommand)]
        command: AssertionCommands,
    },

    #[command(about = "Review inferred assistant task assertions.")]
    TaskAssertions {
        #[command(subcommand)]
        command: TaskAssertionCommands,
    },

    #[command(about = "Manage executive assistant tasks.")]
    Tasks {
        #[command(subcommand)]
        command: TaskCommands,
    },

    #[command(about = "Manage executive assistant reminders.")]
    Reminders {
        #[command(subcommand)]
        command: ReminderCommands,
    },

    #[command(about = "Manage executive assistant contacts.")]
    Contacts {
        #[command(subcommand)]
        command: ContactCommands,
    },

    #[command(about = "Manage executive assistant preferences.")]
    Preferences {
        #[command(subcommand)]
        command: PreferenceCommands,
    },

    #[command(about = "Generate executive assistant briefs.")]
    Briefs {
        #[command(subcommand)]
        command: BriefCommands,
    },

    #[command(about = "Manage near-term calendar event context.")]
    Calendar {
        #[command(subcommand)]
        command: CalendarCommands,
    },

    #[command(name = "source-links", about = "Manage canonical provenance links.")]
    SourceLinks {
        #[command(subcommand)]
        command: SourceLinkCommands,
    },

    #[command(about = "Print CLI version information.")]
    Version,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommands {
    #[command(about = "Store a bearer token in the local profile config.")]
    Login(AuthLoginArgs),

    #[command(about = "Remove stored auth tokens from the local profile config.")]
    Logout,

    #[command(about = "Print local auth status without exposing tokens.")]
    Status,
}

#[derive(Debug, Args)]
pub struct AuthLoginArgs {
    #[arg(
        long = "access-token",
        value_name = "TOKEN",
        help = "Store a bearer token in the selected profile."
    )]
    pub access_token: Option<String>,

    #[arg(
        long = "refresh-token",
        value_name = "TOKEN",
        help = "Store a refresh token in the selected profile."
    )]
    pub refresh_token: Option<String>,

    #[arg(
        long = "expires-at",
        value_name = "UNIX_SECONDS",
        help = "Access token expiry as a Unix timestamp."
    )]
    pub expires_at: Option<i64>,

    #[arg(
        long = "auth-domain",
        env = "SMESH_AUTH0_DOMAIN",
        value_name = "DOMAIN",
        help = "Auth0 domain to store with the profile."
    )]
    pub auth_domain: Option<String>,

    #[arg(
        long = "auth-client-id",
        env = "SMESH_AUTH0_CLIENT_ID",
        value_name = "CLIENT_ID",
        help = "Auth0 client id to store with the profile."
    )]
    pub auth_client_id: Option<String>,

    #[arg(
        long = "auth-audience",
        env = "SMESH_AUTH0_AUDIENCE",
        value_name = "AUDIENCE",
        help = "Auth0 audience to store with the profile."
    )]
    pub auth_audience: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum AgentCommands {
    #[command(about = "Restore portable agent context into this workspace.")]
    Init(AgentInitArgs),

    #[command(about = "Save portable agent Markdown state from this workspace.")]
    Save(AgentSaveArgs),
}

#[derive(Debug, Args)]
pub struct AgentInitArgs {
    #[arg(value_name = "AGENT_NAME", help = "Canonical agent name to restore.")]
    pub name: String,

    #[arg(
        long = "override",
        action = ArgAction::SetTrue,
        help = "Replace existing local files with mesh-stored artifacts."
    )]
    pub override_existing: bool,
}

#[derive(Debug, Args)]
pub struct AgentSaveArgs {
    #[arg(value_name = "AGENT_NAME", help = "Canonical agent name to save.")]
    pub name: String,
}

#[derive(Debug, Subcommand)]
pub enum JobCommands {
    #[command(about = "Fetch a job status by id.")]
    Get(StatusGetArgs),
}

#[derive(Debug, Subcommand)]
pub enum CaptureCommands {
    #[command(about = "Capture plain text into the selected mesh.")]
    Text(CaptureTextArgs),

    #[command(about = "Upload and capture one file through the capture pipeline.")]
    File(CaptureFileArgs),

    #[command(about = "Fetch capture processing status by id.")]
    Status(StatusGetArgs),
}

#[derive(Debug, Args)]
pub struct CaptureTextArgs {
    #[arg(value_name = "TEXT", num_args = 1.., help = "Text to capture.")]
    pub text: Vec<String>,

    #[arg(long, value_name = "TEXT", help = "Capture instructions.")]
    pub instructions: Option<String>,

    #[arg(
        long = "tag",
        action = ArgAction::Append,
        value_name = "TAG",
        help = "Tag to attach to this capture; repeat or comma-separate."
    )]
    pub tags: Vec<String>,
}

#[derive(Debug, Args)]
pub struct CaptureFileArgs {
    #[arg(value_name = "PATH", help = "Path to the file to upload.")]
    pub path: PathBuf,

    #[arg(long, value_name = "TEXT", help = "Capture instructions.")]
    pub instructions: Option<String>,

    #[arg(
        long = "tag",
        action = ArgAction::Append,
        value_name = "TAG",
        help = "Tag to attach to this capture; repeat or comma-separate."
    )]
    pub tags: Vec<String>,

    #[arg(
        long = "mime-type",
        value_name = "MIME",
        help = "Override the detected MIME type."
    )]
    pub mime_type: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatusGetArgs {
    #[arg(value_name = "ID", help = "Job or capture id to inspect.")]
    pub id: String,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    #[arg(value_name = "QUERY", num_args = 1.., help = "Search query text.")]
    pub query: Vec<String>,

    #[arg(
        long = "top-k",
        default_value_t = 10,
        value_name = "N",
        help = "Maximum number of search results to return."
    )]
    pub top_k: usize,

    #[arg(
        long = "filter",
        short = 'f',
        action = ArgAction::Append,
        value_name = "LABEL",
        help = "Node label/type filter. May be repeated or comma-separated."
    )]
    pub filters: Vec<String>,

    #[arg(
        long = "date-from",
        value_name = "ISO_DATE",
        help = "Restrict search to results on or after this date."
    )]
    pub date_from: Option<String>,

    #[arg(
        long = "date-to",
        value_name = "ISO_DATE",
        help = "Restrict search to results on or before this date."
    )]
    pub date_to: Option<String>,
}

#[derive(Debug, Args)]
pub struct AskArgs {
    #[arg(value_name = "QUESTION", num_args = 1.., help = "Question to ask.")]
    pub question: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum TopicCommands {
    #[command(about = "Query topic-linked snippets.")]
    Query(TopicQueryArgs),

    #[command(about = "Summarize topic-linked activity.")]
    Activity(TopicActivityArgs),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum TopicMatch {
    Any,
    All,
}

impl TopicMatch {
    fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::All => "all",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum TopicSort {
    Relevance,
    Recent,
}

impl TopicSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Recent => "recent",
        }
    }
}

#[derive(Debug, Args)]
pub struct TopicCommonArgs {
    #[arg(
        long = "topic",
        short = 't',
        action = ArgAction::Append,
        value_name = "TOPIC",
        help = "Topic token to retrieve. May be repeated or comma-separated."
    )]
    pub topics: Vec<String>,

    #[arg(
        long = "match",
        value_enum,
        default_value_t = TopicMatch::Any,
        value_name = "MODE",
        help = "Whether any or all provided topics must match."
    )]
    pub match_mode: TopicMatch,

    #[arg(
        long = "exclude-node-type",
        alias = "exclude-node-types",
        action = ArgAction::Append,
        value_name = "TYPE",
        help = "Node type to exclude. May be repeated or comma-separated."
    )]
    pub exclude_node_types: Vec<String>,

    #[arg(
        long,
        value_name = "ISO_DATE",
        help = "Start of topic activity window."
    )]
    pub since: Option<String>,

    #[arg(long, value_name = "ISO_DATE", help = "End of topic activity window.")]
    pub until: Option<String>,
}

#[derive(Debug, Args)]
pub struct TopicQueryArgs {
    #[command(flatten)]
    pub common: TopicCommonArgs,

    #[arg(
        long,
        default_value_t = 25,
        value_name = "N",
        help = "Maximum number of topic snippets to return."
    )]
    pub limit: usize,

    #[arg(
        long,
        default_value_t = 0,
        value_name = "N",
        help = "Number of topic snippets to skip."
    )]
    pub offset: usize,

    #[arg(
        long,
        value_enum,
        default_value_t = TopicSort::Relevance,
        value_name = "ORDER",
        help = "Sort topic snippets by relevance or recency."
    )]
    pub sort: TopicSort,

    #[arg(
        long,
        action = ArgAction::SetTrue,
        help = "Shortcut for --sort recent."
    )]
    pub recent: bool,
}

#[derive(Debug, Args)]
pub struct TopicActivityArgs {
    #[command(flatten)]
    pub common: TopicCommonArgs,
}

#[derive(Debug, Args)]
pub struct AgentWriteArgs {
    #[arg(
        long,
        value_name = "TYPE:ID",
        help = "Business actor, e.g. agent:pixel."
    )]
    pub actor: Option<String>,

    #[arg(long = "idempotency-key", value_name = "KEY")]
    pub idempotency_key: Option<String>,

    #[arg(long = "preference-snapshot-id", value_name = "ID")]
    pub preference_snapshot_id: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommands {
    #[command(about = "Create a canonical project.")]
    Create(ProjectCreateArgs),

    #[command(about = "List canonical projects.")]
    List(ProjectListArgs),

    #[command(about = "Fetch a canonical project.")]
    Get(StatusGetArgs),

    #[command(about = "Update a canonical project.")]
    Update(ProjectUpdateArgs),

    #[command(about = "Archive a canonical project.")]
    Archive(ProjectArchiveArgs),
}

#[derive(Debug, Subcommand)]
pub enum AssertionCommands {
    #[command(about = "List assertions awaiting or past review.")]
    List(AssertionListArgs),

    #[command(about = "Fetch an assertion.")]
    Get(StatusGetArgs),

    #[command(about = "Confirm a project assertion as a new project.")]
    Confirm(AssertionConfirmArgs),

    #[command(about = "Deny a project assertion.")]
    Deny(AssertionDenyArgs),

    #[command(about = "Merge a project assertion into an existing project.")]
    Merge(AssertionMergeArgs),

    #[command(about = "Attach a project assertion to an existing project.")]
    Attach(AssertionMergeArgs),

    #[command(about = "Delegate assertion review.")]
    Delegate(AssertionDelegateArgs),
}

#[derive(Debug, Args)]
pub struct AssertionListArgs {
    #[arg(long, value_name = "KIND", default_value = "project")]
    pub kind: String,

    #[arg(long, action = ArgAction::Append, value_name = "STATUS")]
    pub status: Vec<String>,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,

    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: usize,
}

#[derive(Debug, Args)]
pub struct AssertionConfirmArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub summary: Option<String>,

    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    #[arg(long, value_name = "VISIBILITY")]
    pub visibility: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct AssertionDenyArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct AssertionMergeArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long = "target-project-id", value_name = "PROJECT_ID")]
    pub target_project_id: String,

    #[arg(
        long = "merge-mode",
        default_value = "attach_only",
        value_name = "MODE"
    )]
    pub merge_mode: String,

    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub summary: Option<String>,

    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    #[arg(long, value_name = "VISIBILITY")]
    pub visibility: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct AssertionDelegateArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long = "to-agent", value_name = "AGENT_ID")]
    pub to_agent: Option<String>,

    #[arg(long = "to-user", value_name = "USER_ID")]
    pub to_user: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ProjectCreateArgs {
    #[arg(value_name = "TITLE", num_args = 1..)]
    pub title: Vec<String>,

    #[arg(long, value_name = "TEXT")]
    pub summary: Option<String>,

    #[arg(long, value_name = "STATUS", default_value = "active")]
    pub status: String,

    #[arg(long, value_name = "VISIBILITY", default_value = "private")]
    pub visibility: String,

    #[arg(long = "tag", action = ArgAction::Append, value_name = "TAG")]
    pub tags: Vec<String>,

    #[arg(long = "source-type", value_name = "TYPE")]
    pub source_type: Option<String>,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ProjectListArgs {
    #[arg(long, action = ArgAction::Append, value_name = "STATUS")]
    pub status: Vec<String>,

    #[arg(long, action = ArgAction::Append, value_name = "VISIBILITY")]
    pub visibility: Vec<String>,

    #[arg(long = "query", value_name = "TEXT")]
    pub query: Option<String>,

    #[arg(long = "include-archived", action = ArgAction::SetTrue)]
    pub include_archived: bool,

    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,

    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: usize,
}

#[derive(Debug, Args)]
pub struct ProjectUpdateArgs {
    #[arg(value_name = "PROJECT_ID")]
    pub id: String,

    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub summary: Option<String>,

    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    #[arg(long, value_name = "VISIBILITY")]
    pub visibility: Option<String>,

    #[arg(long = "tag", action = ArgAction::Append, value_name = "TAG")]
    pub tags: Vec<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ProjectArchiveArgs {
    #[arg(value_name = "PROJECT_ID")]
    pub id: String,

    #[arg(long, action = ArgAction::SetTrue)]
    pub confirm: bool,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Subcommand)]
pub enum TaskAssertionCommands {
    #[command(about = "List task assertions awaiting or past review.")]
    List(TaskAssertionListArgs),

    #[command(about = "Fetch a task assertion.")]
    Get(StatusGetArgs),

    #[command(about = "Confirm a task assertion as a new canonical task.")]
    Confirm(TaskAssertionConfirmArgs),

    #[command(about = "Deny a task assertion.")]
    Deny(TaskAssertionDenyArgs),

    #[command(about = "Merge a task assertion into an existing task.")]
    Merge(TaskAssertionMergeArgs),

    #[command(about = "Delegate task assertion review.")]
    Delegate(TaskAssertionDelegateArgs),

    #[command(
        name = "attach-project",
        about = "Attach a task assertion to a project or project assertion."
    )]
    AttachProject(TaskAssertionAttachProjectArgs),

    #[command(
        name = "bulk-review",
        about = "Apply several task assertion review actions in one request."
    )]
    BulkReview(TaskAssertionBulkReviewArgs),
}

#[derive(Debug, Args)]
pub struct TaskAssertionListArgs {
    #[arg(long, action = ArgAction::Append, value_name = "STATUS")]
    pub status: Vec<String>,

    #[arg(long = "asserted-by", value_name = "ACTOR_ID")]
    pub asserted_by: Option<String>,

    #[arg(long = "confidence-gte", value_name = "N")]
    pub confidence_gte: Option<f64>,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long = "project-assertion-id", value_name = "ASSERTION_ID")]
    pub project_assertion_id: Option<String>,

    #[arg(long = "source-type", value_name = "TYPE")]
    pub source_type: Option<String>,

    #[arg(long = "due-before", value_name = "ISO_TIMESTAMP")]
    pub due_before: Option<String>,

    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,

    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: usize,
}

#[derive(Debug, Args)]
pub struct TaskAssertionTaskOverridesArgs {
    #[arg(long = "task-id", value_name = "TASK_ID")]
    pub task_id: Option<String>,

    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,

    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    #[arg(long, value_name = "PRIORITY")]
    pub priority: Option<String>,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long = "due-at", value_name = "ISO_TIMESTAMP")]
    pub due_at: Option<String>,

    #[arg(long = "start-at", value_name = "ISO_TIMESTAMP")]
    pub start_at: Option<String>,

    #[arg(long, value_name = "TZ")]
    pub timezone: Option<String>,

    #[arg(long, value_name = "RRULE_OR_JSON")]
    pub recurrence: Option<String>,

    #[arg(long = "assignee-user-id", value_name = "USER_ID")]
    pub assignee_user_id: Option<String>,

    #[arg(long, value_name = "VISIBILITY")]
    pub visibility: Option<String>,

    #[arg(long = "stale-at", value_name = "ISO_TIMESTAMP")]
    pub stale_at: Option<String>,

    #[arg(long = "tag", action = ArgAction::Append, value_name = "TAG")]
    pub tags: Vec<String>,
}

#[derive(Debug, Args)]
pub struct TaskAssertionConfirmArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[command(flatten)]
    pub overrides: TaskAssertionTaskOverridesArgs,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskAssertionDenyArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskAssertionMergeArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long = "task-id", value_name = "TASK_ID")]
    pub task_id: String,

    #[command(flatten)]
    pub overrides: TaskAssertionTaskOverridesArgs,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskAssertionDelegateArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long = "to-agent", value_name = "AGENT_ID")]
    pub to_agent: Option<String>,

    #[arg(long = "to-user", value_name = "USER_ID")]
    pub to_user: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskAssertionAttachProjectArgs {
    #[arg(value_name = "ASSERTION_ID")]
    pub id: String,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long = "project-assertion-id", value_name = "ASSERTION_ID")]
    pub project_assertion_id: Option<String>,

    #[arg(long, action = ArgAction::SetTrue)]
    pub clear: bool,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskAssertionBulkReviewArgs {
    #[arg(long, value_name = "PATH")]
    pub input: Option<PathBuf>,

    #[arg(long, action = ArgAction::SetTrue)]
    pub stdin: bool,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Subcommand)]
pub enum TaskCommands {
    #[command(about = "Create an assistant task.")]
    Create(TaskCreateArgs),

    #[command(about = "List assistant tasks.")]
    List(TaskListArgs),

    #[command(about = "Fetch an assistant task.")]
    Get(StatusGetArgs),

    #[command(about = "Update an assistant task.")]
    Update(TaskUpdateArgs),

    #[command(about = "Complete an assistant task.")]
    Complete(TaskCompleteArgs),

    #[command(about = "Delegate an assistant task.")]
    Delegate(TaskDelegateArgs),

    #[command(name = "attach-source", about = "Attach a source reference to a task.")]
    AttachSource(TaskAttachSourceArgs),
}

#[derive(Debug, Args)]
pub struct TaskCreateArgs {
    #[arg(value_name = "TITLE", num_args = 1.., help = "Task title.")]
    pub title: Vec<String>,

    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,

    #[arg(long, value_name = "STATUS", default_value = "backlog")]
    pub status: String,

    #[arg(long, value_name = "PRIORITY", default_value = "normal")]
    pub priority: String,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long = "due-at", value_name = "ISO_TIMESTAMP")]
    pub due_at: Option<String>,

    #[arg(long = "start-at", value_name = "ISO_TIMESTAMP")]
    pub start_at: Option<String>,

    #[arg(long, value_name = "TZ")]
    pub timezone: Option<String>,

    #[arg(long, value_name = "RRULE_OR_JSON")]
    pub recurrence: Option<String>,

    #[arg(long = "assignee-user-id", value_name = "USER_ID")]
    pub assignee_user_id: Option<String>,

    #[arg(long, value_name = "VISIBILITY", default_value = "private")]
    pub visibility: String,

    #[arg(long = "stale-at", value_name = "ISO_TIMESTAMP")]
    pub stale_at: Option<String>,

    #[arg(long = "tag", action = ArgAction::Append, value_name = "TAG")]
    pub tags: Vec<String>,

    #[arg(long = "source-type", value_name = "TYPE")]
    pub source_type: Option<String>,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskListArgs {
    #[arg(long, action = ArgAction::Append, value_name = "STATUS")]
    pub status: Vec<String>,

    #[arg(long = "due-before", value_name = "ISO_TIMESTAMP")]
    pub due_before: Option<String>,

    #[arg(long = "due-after", value_name = "ISO_TIMESTAMP")]
    pub due_after: Option<String>,

    #[arg(long = "stale-before", value_name = "ISO_TIMESTAMP")]
    pub stale_before: Option<String>,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long = "assignee-user-id", value_name = "USER_ID")]
    pub assignee_user_id: Option<String>,

    #[arg(long = "tag", action = ArgAction::Append, value_name = "TAG")]
    pub tags: Vec<String>,

    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,

    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: usize,
}

#[derive(Debug, Args)]
pub struct TaskUpdateArgs {
    #[arg(value_name = "TASK_ID")]
    pub id: String,

    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,

    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    #[arg(long, value_name = "PRIORITY")]
    pub priority: Option<String>,

    #[arg(long = "project-id", value_name = "PROJECT_ID")]
    pub project_id: Option<String>,

    #[arg(long = "due-at", value_name = "ISO_TIMESTAMP")]
    pub due_at: Option<String>,

    #[arg(long = "start-at", value_name = "ISO_TIMESTAMP")]
    pub start_at: Option<String>,

    #[arg(long, value_name = "TZ")]
    pub timezone: Option<String>,

    #[arg(long, value_name = "RRULE_OR_JSON")]
    pub recurrence: Option<String>,

    #[arg(long = "assignee-user-id", value_name = "USER_ID")]
    pub assignee_user_id: Option<String>,

    #[arg(long = "stale-at", value_name = "ISO_TIMESTAMP")]
    pub stale_at: Option<String>,

    #[arg(long = "tag", action = ArgAction::Append, value_name = "TAG")]
    pub tags: Vec<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskCompleteArgs {
    #[arg(value_name = "TASK_ID")]
    pub id: String,

    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskDelegateArgs {
    #[arg(value_name = "TASK_ID")]
    pub id: String,

    #[arg(long = "to-agent", value_name = "AGENT_ID")]
    pub to_agent: Option<String>,

    #[arg(long = "to-user", value_name = "USER_ID")]
    pub to_user: Option<String>,

    #[arg(long = "to-contact", value_name = "CONTACT_ID")]
    pub to_contact: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct TaskAttachSourceArgs {
    #[arg(value_name = "TASK_ID")]
    pub id: String,

    #[arg(long = "source-type", value_name = "TYPE")]
    pub source_type: String,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: String,

    #[arg(long = "span-start", value_name = "N")]
    pub span_start: Option<i64>,

    #[arg(long = "span-end", value_name = "N")]
    pub span_end: Option<i64>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Subcommand)]
pub enum ReminderCommands {
    #[command(about = "Create a reminder.")]
    Create(ReminderCreateArgs),

    #[command(about = "List reminders.")]
    List(ReminderListArgs),

    #[command(name = "due-soon", about = "List reminders due soon.")]
    DueSoon(ReminderDueSoonArgs),

    #[command(about = "Snooze a reminder.")]
    Snooze(ReminderSnoozeArgs),

    #[command(about = "Complete a reminder.")]
    Complete(ReminderCompleteArgs),

    #[command(about = "Dismiss a reminder.")]
    Dismiss(StatusGetArgs),
}

#[derive(Debug, Args)]
pub struct ReminderCreateArgs {
    #[arg(value_name = "TITLE", num_args = 1..)]
    pub title: Vec<String>,

    #[arg(long = "task-id", value_name = "TASK_ID")]
    pub task_id: Option<String>,

    #[arg(long, value_name = "STRATEGY", default_value = "absolute")]
    pub strategy: String,

    #[arg(long = "absolute-at", value_name = "ISO_TIMESTAMP")]
    pub absolute_at: Option<String>,

    #[arg(long = "relative-to", value_name = "FIELD")]
    pub relative_to: Option<String>,

    #[arg(long, value_name = "ISO_DURATION")]
    pub offset: Option<String>,

    #[arg(long, value_name = "RRULE_OR_JSON")]
    pub schedule: Option<String>,

    #[arg(long, value_name = "CHANNEL", default_value = "in_app")]
    pub channel: String,
}

#[derive(Debug, Args)]
pub struct ReminderListArgs {
    #[arg(long = "task-id", value_name = "TASK_ID")]
    pub task_id: Option<String>,

    #[arg(long = "due-before", value_name = "ISO_TIMESTAMP")]
    pub due_before: Option<String>,

    #[arg(long = "due-after", value_name = "ISO_TIMESTAMP")]
    pub due_after: Option<String>,

    #[arg(long, action = ArgAction::Append, value_name = "STATE")]
    pub state: Vec<String>,

    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,

    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: usize,
}

#[derive(Debug, Args)]
pub struct ReminderDueSoonArgs {
    #[arg(long, default_value = "PT24H", value_name = "ISO_DURATION")]
    pub window: String,

    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct ReminderSnoozeArgs {
    #[arg(value_name = "REMINDER_ID")]
    pub id: String,

    #[arg(long, value_name = "ISO_TIMESTAMP")]
    pub until: Option<String>,

    #[arg(long, value_name = "ISO_DURATION")]
    pub duration: Option<String>,
}

#[derive(Debug, Args)]
pub struct ReminderCompleteArgs {
    #[arg(value_name = "REMINDER_ID")]
    pub id: String,

    #[arg(long = "complete-task", action = ArgAction::SetTrue)]
    pub complete_task: bool,
}

#[derive(Debug, Subcommand)]
pub enum ContactCommands {
    #[command(about = "Manage people contacts.")]
    People {
        #[command(subcommand)]
        command: ContactPeopleCommands,
    },

    #[command(about = "Manage organization contacts.")]
    Orgs {
        #[command(subcommand)]
        command: ContactOrgCommands,
    },

    #[command(about = "Manage contact source links.")]
    Links {
        #[command(subcommand)]
        command: ContactLinkCommands,
    },

    #[command(about = "Manage contact relationships.")]
    Relationships {
        #[command(subcommand)]
        command: ContactRelationshipCommands,
    },

    #[command(name = "open-loops", about = "List open loops by contact.")]
    OpenLoops {
        #[command(subcommand)]
        command: ContactOpenLoopCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ContactPeopleCommands {
    Create(ContactPersonCreateArgs),
    List(ContactListArgs),
    Get(ContactGetArgs),
    Update(ContactPersonUpdateArgs),
    Archive(ContactArchiveArgs),
    Merge(ContactMergeArgs),
    Note {
        #[command(subcommand)]
        command: ContactNoteCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ContactNoteCommands {
    Add(ContactNoteAddArgs),
}

#[derive(Debug, Subcommand)]
pub enum ContactOrgCommands {
    Create(ContactOrgCreateArgs),
    List(ContactListArgs),
    Get(StatusGetArgs),
    Update(ContactOrgUpdateArgs),
    Archive(ContactArchiveArgs),
    Merge(ContactMergeArgs),
}

#[derive(Debug, Subcommand)]
pub enum ContactLinkCommands {
    Add(ContactLinkAddArgs),
}

#[derive(Debug, Subcommand)]
pub enum ContactRelationshipCommands {
    Add(ContactRelationshipAddArgs),
    List(ContactRelationshipListArgs),
    Remove(ContactRelationshipRemoveArgs),
}

#[derive(Debug, Subcommand)]
pub enum ContactOpenLoopCommands {
    List(ContactOpenLoopsListArgs),
}

#[derive(Debug, Args)]
pub struct ContactPersonCreateArgs {
    #[arg(long = "name", value_name = "NAME")]
    pub name: String,

    #[arg(long = "org", value_name = "ORG_ID")]
    pub org: Option<String>,

    #[arg(long, value_name = "EMAIL")]
    pub email: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactOrgCreateArgs {
    #[arg(long = "name", value_name = "NAME")]
    pub name: String,

    #[arg(long, value_name = "DOMAIN")]
    pub domain: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactListArgs {
    #[arg(long = "query", value_name = "TEXT")]
    pub query: Option<String>,

    #[arg(long = "open-loops", action = ArgAction::SetTrue)]
    pub open_loops: bool,

    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,

    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: usize,
}

#[derive(Debug, Args)]
pub struct ContactGetArgs {
    #[arg(value_name = "PERSON_ID")]
    pub id: String,

    #[arg(long, value_name = "FIELDS")]
    pub include: Option<String>,
}

#[derive(Debug, Args)]
pub struct ContactPersonUpdateArgs {
    #[arg(value_name = "PERSON_ID")]
    pub id: String,

    #[arg(long = "name", value_name = "NAME")]
    pub name: Option<String>,

    #[arg(long = "email", value_name = "EMAIL")]
    pub email: Option<String>,

    #[arg(long = "org", value_name = "ORG_ID")]
    pub org: Option<String>,

    #[arg(long = "role", value_name = "TEXT")]
    pub role: Option<String>,

    #[arg(long = "timezone", value_name = "TZ")]
    pub timezone: Option<String>,

    #[arg(long = "preferred-channel", value_name = "CHANNEL")]
    pub preferred_channel: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactOrgUpdateArgs {
    #[arg(value_name = "ORG_ID")]
    pub id: String,

    #[arg(long = "name", value_name = "NAME")]
    pub name: Option<String>,

    #[arg(long = "domain", value_name = "DOMAIN")]
    pub domain: Option<String>,

    #[arg(long = "website", value_name = "URL")]
    pub website: Option<String>,

    #[arg(long = "summary", value_name = "TEXT")]
    pub summary: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactArchiveArgs {
    #[arg(value_name = "CONTACT_ID")]
    pub id: String,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactMergeArgs {
    #[arg(value_name = "CONTACT_ID")]
    pub id: String,

    #[arg(long = "into", value_name = "CONTACT_ID")]
    pub into: String,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactNoteAddArgs {
    #[arg(value_name = "PERSON_ID")]
    pub id: String,

    #[arg(long = "body", value_name = "TEXT")]
    pub body: String,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactLinkAddArgs {
    #[arg(value_name = "CONTACT_ID")]
    pub id: String,

    #[arg(long = "source-type", value_name = "TYPE")]
    pub source_type: String,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: String,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactRelationshipAddArgs {
    #[arg(long = "from", value_name = "CONTACT_ID")]
    pub from_contact_id: String,

    #[arg(long = "to", value_name = "TARGET_ID")]
    pub to: String,

    #[arg(long = "type", value_name = "RELATIONSHIP_TYPE")]
    pub relationship_type: String,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactRelationshipListArgs {
    #[arg(value_name = "CONTACT_ID")]
    pub id: String,
}

#[derive(Debug, Args)]
pub struct ContactRelationshipRemoveArgs {
    #[arg(value_name = "RELATIONSHIP_ID")]
    pub id: String,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct ContactOpenLoopsListArgs {
    #[arg(long, value_name = "PERSON_ID")]
    pub person: Option<String>,

    #[arg(long, value_name = "ORG_ID")]
    pub org: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum PreferenceCommands {
    #[command(about = "Set a scoped preference.")]
    Set(PreferenceSetArgs),
    #[command(about = "List preferences.")]
    List(PreferenceListArgs),
    #[command(about = "Resolve effective preferences for the current work context.")]
    Resolve(PreferenceResolveArgs),
    #[command(about = "Fetch a preference.")]
    Get(StatusGetArgs),
    #[command(about = "Confirm a preference.")]
    Confirm(PreferenceConfirmArgs),
    #[command(about = "Revoke a preference.")]
    Revoke(PreferenceRevokeArgs),
    #[command(about = "List preference evidence.")]
    Evidence(StatusGetArgs),
}

#[derive(Debug, Args)]
pub struct PreferenceSetArgs {
    #[arg(long, value_name = "SCOPE")]
    pub scope: String,

    #[arg(long, value_name = "KEY")]
    pub key: String,

    #[arg(long, value_name = "VALUE")]
    pub value: Option<String>,

    #[arg(long = "value-json", value_name = "JSON")]
    pub value_json: Option<String>,

    #[arg(long = "value-type", value_name = "TYPE", default_value = "string")]
    pub value_type: String,

    #[arg(long, value_name = "POLARITY", default_value = "need")]
    pub polarity: String,

    #[arg(long, value_name = "ENFORCEMENT")]
    pub enforcement: Option<String>,

    #[arg(long, value_name = "SENSITIVITY")]
    pub sensitivity: Option<String>,

    #[arg(long = "surface", value_name = "SURFACE")]
    pub surface: Vec<String>,

    #[arg(long = "applies-to-tools", value_name = "TOOL")]
    pub applies_to_tools: Vec<String>,

    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    #[arg(long = "confirmation-state", value_name = "STATE")]
    pub confirmation_state: Option<String>,

    #[arg(long, value_name = "N", default_value_t = 1.0)]
    pub confidence: f64,

    #[arg(long = "source-type", value_name = "TYPE")]
    pub source_type: Option<String>,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: Option<String>,

    #[arg(
        long = "update-rule",
        value_name = "RULE",
        default_value = "append_evidence"
    )]
    pub update_rule: String,

    #[arg(long = "stale-at", value_name = "ISO_TIMESTAMP")]
    pub stale_at: Option<String>,

    #[arg(long = "conflicts-with", value_name = "PREFERENCE_ID")]
    pub conflicts_with: Vec<String>,

    #[arg(long = "supersedes", value_name = "PREFERENCE_ID")]
    pub supersedes: Vec<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct PreferenceListArgs {
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,

    #[arg(long, value_name = "DOMAIN")]
    pub domain: Option<String>,

    #[arg(long, value_name = "SURFACE")]
    pub surface: Option<String>,

    #[arg(long, value_name = "TOOL")]
    pub tool: Option<String>,

    #[arg(long = "status", value_name = "STATUS")]
    pub status: Vec<String>,

    #[arg(long = "min-confidence", value_name = "N", default_value_t = 0.0)]
    pub min_confidence: f64,

    #[arg(long, default_value_t = 100, value_name = "N")]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct PreferenceResolveArgs {
    #[arg(long, value_name = "TYPE:ID")]
    pub actor: String,

    #[arg(long, value_name = "SURFACE")]
    pub surface: String,

    #[arg(long = "tool", value_name = "TOOL")]
    pub tools: Vec<String>,

    #[arg(long, value_name = "ACTION")]
    pub action: Option<String>,

    #[arg(long = "target", value_name = "TYPE:ID")]
    pub targets: Vec<String>,

    #[arg(long = "current-instruction-ref", value_name = "REF")]
    pub current_instruction_refs: Vec<String>,

    #[arg(long = "include-evidence", action = ArgAction::SetTrue)]
    pub include_evidence: bool,
}

#[derive(Debug, Args)]
pub struct PreferenceConfirmArgs {
    #[arg(value_name = "PREFERENCE_ID")]
    pub id: String,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct PreferenceRevokeArgs {
    #[arg(value_name = "PREFERENCE_ID")]
    pub id: String,

    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,

    #[arg(long, action = ArgAction::SetTrue)]
    pub confirm: bool,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Subcommand)]
pub enum BriefCommands {
    Daily(BriefDailyArgs),
    Project(BriefProjectArgs),
    Contact(BriefContactArgs),
    #[command(name = "meeting-prep")]
    MeetingPrep(BriefMeetingPrepArgs),
    #[command(name = "what-changed")]
    WhatChanged(BriefWhatChangedArgs),
}

#[derive(Debug, Args)]
pub struct BriefDailyArgs {
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,

    #[arg(long, action = ArgAction::SetTrue)]
    pub refresh: bool,
}

#[derive(Debug, Args)]
pub struct BriefProjectArgs {
    #[arg(value_name = "PROJECT_ID")]
    pub project_id: String,

    #[arg(long, value_name = "ISO_TIMESTAMP")]
    pub since: Option<String>,

    #[arg(long, action = ArgAction::SetTrue)]
    pub refresh: bool,
}

#[derive(Debug, Args)]
pub struct BriefContactArgs {
    #[arg(value_name = "CONTACT_ID")]
    pub contact_id: String,

    #[arg(long, action = ArgAction::SetTrue)]
    pub refresh: bool,
}

#[derive(Debug, Args)]
pub struct BriefMeetingPrepArgs {
    #[arg(value_name = "EVENT_ID")]
    pub event_id: String,

    #[arg(long, action = ArgAction::SetTrue)]
    pub refresh: bool,
}

#[derive(Debug, Args)]
pub struct BriefWhatChangedArgs {
    #[arg(long, value_name = "ISO_TIMESTAMP")]
    pub since: Option<String>,

    #[arg(long, value_name = "SCOPE", default_value = "mesh")]
    pub scope: String,
}

#[derive(Debug, Subcommand)]
pub enum CalendarCommands {
    Events {
        #[command(subcommand)]
        command: CalendarEventCommands,
    },
    #[command(name = "meeting-prep")]
    MeetingPrep(BriefMeetingPrepArgs),
}

#[derive(Debug, Subcommand)]
pub enum CalendarEventCommands {
    List(CalendarEventListArgs),
    Get(StatusGetArgs),
    Upsert(CalendarEventUpsertArgs),
}

#[derive(Debug, Args)]
pub struct CalendarEventListArgs {
    #[arg(long = "from", value_name = "ISO_TIMESTAMP")]
    pub from_ts: String,

    #[arg(long = "to", value_name = "ISO_TIMESTAMP")]
    pub to_ts: String,

    #[arg(long = "include-attendees", action = ArgAction::SetTrue)]
    pub include_attendees: bool,

    #[arg(long, default_value_t = 100, value_name = "N")]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct CalendarEventUpsertArgs {
    #[arg(long = "file", value_name = "JSON_FILE")]
    pub file: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum SourceLinkCommands {
    #[command(about = "Add a canonical source link.")]
    Add(SourceLinkAddArgs),

    #[command(about = "List canonical source links.")]
    List(SourceLinkListArgs),

    #[command(about = "Remove a canonical source link.")]
    Remove(SourceLinkRemoveArgs),
}

#[derive(Debug, Args)]
pub struct SourceLinkAddArgs {
    #[arg(long = "target-type", value_name = "TYPE")]
    pub target_type: String,

    #[arg(long = "target-id", value_name = "ID")]
    pub target_id: String,

    #[arg(long = "source-type", value_name = "TYPE")]
    pub source_type: String,

    #[arg(long = "source-id", value_name = "ID")]
    pub source_id: String,

    #[arg(long, value_name = "LABEL")]
    pub label: Option<String>,

    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Args)]
pub struct SourceLinkListArgs {
    #[arg(long = "target-type", value_name = "TYPE")]
    pub target_type: Option<String>,

    #[arg(long = "target-id", value_name = "ID")]
    pub target_id: Option<String>,

    #[arg(long = "include-removed", action = ArgAction::SetTrue)]
    pub include_removed: bool,

    #[arg(long, default_value_t = 100, value_name = "N")]
    pub limit: usize,

    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: usize,
}

#[derive(Debug, Args)]
pub struct SourceLinkRemoveArgs {
    #[arg(value_name = "SOURCE_LINK_ID")]
    pub id: String,

    #[arg(long, action = ArgAction::SetTrue)]
    pub confirm: bool,

    #[command(flatten)]
    pub write: AgentWriteArgs,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("failed to serialize CLI output: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("{message}")]
    Command {
        message: String,
        exit_code: i32,
        status: Option<u16>,
        details: Option<Value>,
    },
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Serialize(_) => 1,
            Self::Command { exit_code, .. } => *exit_code,
        }
    }

    pub fn render(&self, mode: OutputMode) -> Result<String, serde_json::Error> {
        let error = MachineErrorOutput {
            event_type: match mode {
                OutputMode::Ndjson => Some("error"),
                OutputMode::Human | OutputMode::Json => None,
            },
            error: self.machine_message(),
            status: self.status(),
            details: self.details().cloned(),
        };

        Ok(format!("{}\n", serde_json::to_string(&error)?))
    }

    fn machine_message(&self) -> String {
        match self {
            Self::Serialize(error) => format!("failed to serialize CLI output: {error}"),
            Self::Command { message, .. } => message.clone(),
        }
    }

    fn status(&self) -> Option<u16> {
        match self {
            Self::Serialize(_) => None,
            Self::Command { status, .. } => *status,
        }
    }

    fn details(&self) -> Option<&Value> {
        match self {
            Self::Serialize(_) => None,
            Self::Command { details, .. } => details.as_ref(),
        }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Command {
            message: message.into(),
            exit_code: 2,
            status: None,
            details: None,
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self::Command {
            message: message.into(),
            exit_code: 6,
            status: None,
            details: None,
        }
    }

    fn auth_required(message: impl Into<String>) -> Self {
        Self::Command {
            message: message.into(),
            exit_code: 4,
            status: Some(401),
            details: None,
        }
    }

    fn http(status: u16, message: impl Into<String>, details: Option<Value>) -> Self {
        Self::Command {
            message: message.into(),
            exit_code: if status == 404 { 5 } else { 1 },
            status: Some(status),
            details,
        }
    }

    fn network(message: impl Into<String>) -> Self {
        Self::Command {
            message: message.into(),
            exit_code: 1,
            status: None,
            details: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct MachineErrorOutput {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    event_type: Option<&'static str>,
    error: String,
    status: Option<u16>,
    details: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VersionOutput<'a> {
    pub binary: &'a str,
    pub package: &'a str,
    pub version: &'a str,
}

impl VersionOutput<'static> {
    pub fn current() -> Self {
        Self {
            binary: BINARY_NAME,
            package: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct VersionEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'a str,
    #[serde(flatten)]
    version: VersionOutput<'a>,
}

#[derive(Debug, Serialize)]
struct SearchOutput {
    schema_version: u8,
    query: String,
    top_k: usize,
    filters: SearchOutputFilters,
    result_count: usize,
    results: Vec<SearchResultOutput>,
}

#[derive(Debug, Serialize)]
struct SearchOutputFilters {
    labels: Vec<String>,
    date_from: Option<String>,
    date_to: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchResultOutput {
    rank: usize,
    id: Option<String>,
    node_type: String,
    title: Option<String>,
    snippet: Option<String>,
    score: Option<f64>,
    distance: Option<f64>,
    data: Value,
}

#[derive(Debug, Serialize)]
struct AskOutput {
    schema_version: u8,
    question: String,
    answer: String,
    source_ids: Vec<String>,
    sources: Vec<Value>,
    report_ids: Vec<String>,
    suggested_title: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct TopicWindowOutput {
    since: Option<String>,
    until: Option<String>,
}

#[derive(Debug, Serialize)]
struct TopicQueryOutput {
    schema_version: u8,
    topics: Vec<String>,
    #[serde(rename = "match")]
    match_mode: String,
    limit: usize,
    offset: usize,
    sort: String,
    window: TopicWindowOutput,
    exclude_node_types: Vec<String>,
    items: Vec<TopicItemOutput>,
    paging: TopicPagingOutput,
}

#[derive(Debug, Serialize)]
struct TopicItemOutput {
    id: String,
    node_type: String,
    title: Option<String>,
    snippet: String,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    matched_topics: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TopicPagingOutput {
    limit: usize,
    offset: usize,
    total: usize,
    has_more: bool,
}

#[derive(Debug, Serialize)]
struct TopicActivityOutput {
    schema_version: u8,
    topics: Vec<String>,
    #[serde(rename = "match")]
    match_mode: String,
    window: TopicWindowOutput,
    exclude_node_types: Vec<String>,
    total: usize,
    by_node_type: Vec<TopicActivityRowOutput>,
}

#[derive(Debug, Serialize)]
struct TopicActivityRowOutput {
    node_type: String,
    count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AgentPortableManifest {
    schema_version: u8,
    agent_name: String,
    agent_id: String,
    generated_at_unix_seconds: i64,
    mesh_id: Option<String>,
    workspace: AgentWorkspaceMetadata,
    index: AgentIndexArtifact,
    artifacts: Vec<AgentManifestArtifact>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AgentWorkspaceMetadata {
    path: Option<String>,
    host: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AgentIndexArtifact {
    path: String,
    kind: String,
    format: String,
    content: String,
    sha256: String,
    generated_at_unix_seconds: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AgentManifestArtifact {
    path: String,
    kind: String,
    format: String,
    content: String,
    sha256: String,
    size_bytes: usize,
    captured_at_unix_seconds: i64,
    source: AgentArtifactSource,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AgentArtifactSource {
    workspace_path: Option<String>,
    host: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentFileOutput {
    path: String,
    kind: String,
    sha256: String,
    bytes: usize,
    status: String,
    reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct AgentInitOutput {
    schema_version: u8,
    operation_id: String,
    status: &'static str,
    action: &'static str,
    agent_name: String,
    agent_id: String,
    mesh_id: String,
    index_path: String,
    override_existing: bool,
    created_agent: bool,
    agent_version: Option<String>,
    synapse_task_id: Option<String>,
    artifacts_total: usize,
    restored: Vec<AgentFileOutput>,
    skipped: Vec<AgentFileOutput>,
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct AgentSaveOutput {
    schema_version: u8,
    operation_id: String,
    status: &'static str,
    action: &'static str,
    agent_name: String,
    agent_id: String,
    mesh_id: String,
    index_path: String,
    agent_version: Option<String>,
    synapse_task_id: Option<String>,
    artifacts_saved: usize,
    artifacts: Vec<AgentFileOutput>,
    warnings: Vec<String>,
}

pub fn run(cli: Cli) -> Result<String, CliError> {
    match &cli.command {
        Commands::Auth { command } => render_auth_command(&cli, command),
        Commands::Agent { command } => render_agent_command(&cli, command),
        Commands::Jobs { command } => render_job_command(&cli, command),
        Commands::Capture { command } => render_capture_command(&cli, command),
        Commands::Search(args) => render_search_command(&cli, args),
        Commands::Ask(args) => render_ask_command(&cli, args),
        Commands::Topics { command } => render_topic_command(&cli, command),
        Commands::Projects { command } => render_project_command(&cli, command),
        Commands::Assertions { command } => render_assertion_command(&cli, command),
        Commands::TaskAssertions { command } => render_task_assertion_command(&cli, command),
        Commands::Tasks { command } => render_task_command(&cli, command),
        Commands::Reminders { command } => render_reminder_command(&cli, command),
        Commands::Contacts { command } => render_contact_command(&cli, command),
        Commands::Preferences { command } => render_preference_command(&cli, command),
        Commands::Briefs { command } => render_brief_command(&cli, command),
        Commands::Calendar { command } => render_calendar_command(&cli, command),
        Commands::SourceLinks { command } => render_source_link_command(&cli, command),
        Commands::Version => render_version(cli.effective_output()),
    }
}

pub fn render_version(mode: OutputMode) -> Result<String, CliError> {
    let version = VersionOutput::current();

    match mode {
        OutputMode::Human => Ok(format!("{} {}\n", version.binary, version.version)),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&version)?)),
        OutputMode::Ndjson => {
            let event = VersionEvent {
                event_type: "version",
                version,
            };
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_agent_command(cli: &Cli, command: &AgentCommands) -> Result<String, CliError> {
    match command {
        AgentCommands::Init(args) => render_agent_init(cli, args),
        AgentCommands::Save(args) => render_agent_save(cli, args),
    }
}

fn render_agent_init(cli: &Cli, args: &AgentInitArgs) -> Result<String, CliError> {
    let agent_name = normalize_agent_name(&args.name)?;
    let agent_id = agent_name.clone();
    let workspace_root = current_workspace_dir()?;
    let context = remote_context(cli, true)?;
    let mesh_id = context
        .mesh_id
        .clone()
        .filter(|mesh_id| !mesh_id.is_empty())
        .ok_or_else(|| {
            CliError::config(
                "Missing mesh context. Pass --mesh-id, set SMESH_MESH_ID, or store mesh_id in the selected profile.",
            )
        })?;

    let mut warnings = Vec::new();
    let (agent_response, created_agent) =
        match get_agent_registry_manifest(&context, &agent_id, &mesh_id)? {
            Some(value) => (value, false),
            None => {
                let manifest =
                    empty_agent_manifest(&agent_name, &agent_id, &mesh_id, &workspace_root);
                (
                    set_agent_registry_manifest(&context, &manifest, &mesh_id)?,
                    true,
                )
            }
        };

    let mut manifest = manifest_from_agent_response(
        &agent_response,
        &agent_name,
        &agent_id,
        &mesh_id,
        &workspace_root,
        &mut warnings,
    )?;
    ensure_agent_index(
        &mut manifest,
        &agent_name,
        &agent_id,
        &mesh_id,
        &workspace_root,
    );
    resolve_safe_agent_path(&workspace_root, &manifest.index.path)?;
    for artifact in &manifest.artifacts {
        resolve_safe_agent_path(&workspace_root, &artifact.path)?;
    }

    let mut restored = Vec::new();
    let mut skipped = Vec::new();
    let index_result = write_agent_workspace_file(
        &workspace_root,
        &manifest.index.path,
        &manifest.index.kind,
        &manifest.index.content,
        args.override_existing,
    )?;
    if index_result.status == "skipped" {
        skipped.push(index_result);
    } else {
        restored.push(index_result);
    }

    for artifact in &manifest.artifacts {
        if artifact.format != "md" {
            warnings.push(format!(
                "Skipped non-Markdown artifact `{}` with format `{}`.",
                artifact.path, artifact.format
            ));
            continue;
        }
        let actual_sha = sha256_hex(artifact.content.as_bytes());
        if !artifact.sha256.is_empty() && artifact.sha256 != actual_sha {
            warnings.push(format!(
                "Artifact `{}` content hash differed from manifest metadata.",
                artifact.path
            ));
        }
        let result = write_agent_workspace_file(
            &workspace_root,
            &artifact.path,
            &artifact.kind,
            &artifact.content,
            args.override_existing,
        )?;
        if result.status == "skipped" {
            skipped.push(result);
        } else {
            restored.push(result);
        }
    }

    let output = AgentInitOutput {
        schema_version: AGENT_PORTABLE_SCHEMA_VERSION,
        operation_id: local_operation_id("agent.init"),
        status: "ok",
        action: "agent.init",
        agent_name,
        agent_id,
        mesh_id,
        index_path: manifest.index.path.clone(),
        override_existing: args.override_existing,
        created_agent,
        agent_version: registry_response_string(&agent_response, "version"),
        synapse_task_id: registry_response_string(&agent_response, "synapse_task_id"),
        artifacts_total: manifest.artifacts.len(),
        restored,
        skipped,
        warnings,
    };

    render_agent_output(
        cli.effective_output(),
        "agent.init",
        render_agent_init_human(&output),
        &output,
    )
}

fn render_agent_save(cli: &Cli, args: &AgentSaveArgs) -> Result<String, CliError> {
    let agent_name = normalize_agent_name(&args.name)?;
    let agent_id = agent_name.clone();
    let workspace_root = current_workspace_dir()?;
    let context = remote_context(cli, true)?;
    let mesh_id = context
        .mesh_id
        .clone()
        .filter(|mesh_id| !mesh_id.is_empty())
        .ok_or_else(|| {
            CliError::config(
                "Missing mesh context. Pass --mesh-id, set SMESH_MESH_ID, or store mesh_id in the selected profile.",
            )
        })?;

    let artifacts = discover_agent_markdown_artifacts(&workspace_root)?;
    let mut manifest =
        manifest_from_artifacts(&agent_name, &agent_id, &mesh_id, &workspace_root, artifacts);
    ensure_agent_index(
        &mut manifest,
        &agent_name,
        &agent_id,
        &mesh_id,
        &workspace_root,
    );
    write_generated_agent_index(&workspace_root, &manifest.index)?;

    let response = set_agent_registry_manifest(&context, &manifest, &mesh_id)?;
    set_agent_markdown_artifact(
        &context,
        &agent_id,
        &mesh_id,
        &manifest.index.path,
        &manifest.index.kind,
        &manifest.index.format,
        &manifest.index.content,
    )?;
    for artifact in &manifest.artifacts {
        set_agent_markdown_artifact(
            &context,
            &agent_id,
            &mesh_id,
            &artifact.path,
            &artifact.kind,
            &artifact.format,
            &artifact.content,
        )?;
    }
    let artifact_outputs = manifest
        .artifacts
        .iter()
        .map(|artifact| AgentFileOutput {
            path: artifact.path.clone(),
            kind: artifact.kind.clone(),
            sha256: artifact.sha256.clone(),
            bytes: artifact.size_bytes,
            status: "saved".to_string(),
            reason: None,
        })
        .collect::<Vec<_>>();

    let output = AgentSaveOutput {
        schema_version: AGENT_PORTABLE_SCHEMA_VERSION,
        operation_id: local_operation_id("agent.save"),
        status: "ok",
        action: "agent.save",
        agent_name,
        agent_id,
        mesh_id,
        index_path: manifest.index.path.clone(),
        agent_version: registry_response_string(&response, "version"),
        synapse_task_id: registry_response_string(&response, "synapse_task_id"),
        artifacts_saved: artifact_outputs.len(),
        artifacts: artifact_outputs,
        warnings: Vec::new(),
    };

    render_agent_output(
        cli.effective_output(),
        "agent.save",
        render_agent_save_human(&output),
        &output,
    )
}

fn render_agent_output<T: Serialize>(
    mode: OutputMode,
    event_type: &'static str,
    human: String,
    output: &T,
) -> Result<String, CliError> {
    match mode {
        OutputMode::Human => Ok(human),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(output)?)),
        OutputMode::Ndjson => {
            let mut event = match serde_json::to_value(output)? {
                Value::Object(map) => map,
                other => {
                    let mut map = Map::new();
                    map.insert("payload".to_string(), other);
                    map
                }
            };
            event.insert("type".to_string(), Value::String(event_type.to_string()));
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_agent_init_human(output: &AgentInitOutput) -> String {
    let mut lines = vec![format!("Agent {} initialized.", output.agent_name)];
    lines.push(format!("Index: {}", output.index_path));
    lines.push(format!("Artifacts in mesh: {}", output.artifacts_total));
    lines.push(format!("Restored: {}", output.restored.len()));
    lines.push(format!("Skipped: {}", output.skipped.len()));
    if output.override_existing {
        lines.push("Override: existing local files were eligible for replacement.".to_string());
    }
    if let Some(version) = output.agent_version.as_deref() {
        lines.push(format!("Agent registry version: {version}"));
    }
    for warning in &output.warnings {
        lines.push(format!("Warning: {warning}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn render_agent_save_human(output: &AgentSaveOutput) -> String {
    let mut lines = vec![format!("Agent {} saved.", output.agent_name)];
    lines.push(format!("Index: {}", output.index_path));
    lines.push(format!("Artifacts saved: {}", output.artifacts_saved));
    if let Some(version) = output.agent_version.as_deref() {
        lines.push(format!("Agent registry version: {version}"));
    }
    if let Some(task_id) = output.synapse_task_id.as_deref() {
        lines.push(format!("Projection task: {task_id}"));
    }
    for warning in &output.warnings {
        lines.push(format!("Warning: {warning}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn get_agent_registry_manifest(
    context: &RemoteContext,
    agent_id: &str,
    mesh_id: &str,
) -> Result<Option<Value>, CliError> {
    let query = vec![
        ("agent_id".to_string(), agent_id.to_string()),
        ("mesh_id".to_string(), mesh_id.to_string()),
    ];
    let primary_path = path_with_query("/api/cli/agent/get", &query);
    let fallback_path = path_with_query("/v1/agent/get", &query);

    match get_json(context, &primary_path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if registry_not_found(&error, "agent not found") => Ok(None),
        Err(error) if matches!(error.status(), Some(404 | 405)) => {
            match get_json(context, &fallback_path) {
                Ok(value) => Ok(Some(value)),
                Err(fallback_error) if fallback_error.status() == Some(404) => Ok(None),
                Err(fallback_error) => Err(fallback_error),
            }
        }
        Err(error) => Err(error),
    }
}

fn registry_not_found(error: &CliError, expected_message: &str) -> bool {
    error.status() == Some(404)
        && error
            .machine_message()
            .to_ascii_lowercase()
            .contains(expected_message)
}

fn set_agent_registry_manifest(
    context: &RemoteContext,
    manifest: &AgentPortableManifest,
    mesh_id: &str,
) -> Result<Value, CliError> {
    let content = serde_json::to_string_pretty(manifest)?;
    let payload = json!({
        "agent_id": manifest.agent_id,
        "mesh_id": mesh_id,
        "format": "json",
        "content": content,
    });
    post_json_with_fallback(context, "/api/cli/agent/set", "/v1/agent/set", payload)
}

fn set_agent_context_artifact(
    context: &RemoteContext,
    agent_id: &str,
    mesh_id: &str,
    path: &str,
    format: &str,
    content: &str,
) -> Result<Value, CliError> {
    let payload = json!({
        "agent_id": agent_id,
        "key": path,
        "mesh_id": mesh_id,
        "format": format,
        "content": content,
    });
    post_json_with_fallback(context, "/api/cli/context/set", "/v1/context/set", payload)
}

fn set_agent_skill_artifact(
    context: &RemoteContext,
    agent_id: &str,
    mesh_id: &str,
    path: &str,
    format: &str,
    content: &str,
) -> Result<Value, CliError> {
    let payload = json!({
        "skill_name": path,
        "agent_id": agent_id,
        "mesh_id": mesh_id,
        "format": format,
        "content": content,
    });
    post_json_with_fallback(context, "/api/cli/skills/set", "/v1/skills/set", payload)
}

fn set_agent_markdown_artifact(
    context: &RemoteContext,
    agent_id: &str,
    mesh_id: &str,
    path: &str,
    kind: &str,
    format: &str,
    content: &str,
) -> Result<Value, CliError> {
    match kind {
        "skill" => set_agent_skill_artifact(context, agent_id, mesh_id, path, format, content),
        _ => set_agent_context_artifact(context, agent_id, mesh_id, path, format, content),
    }
}

fn manifest_from_agent_response(
    response: &Value,
    agent_name: &str,
    agent_id: &str,
    mesh_id: &str,
    workspace_root: &Path,
    warnings: &mut Vec<String>,
) -> Result<AgentPortableManifest, CliError> {
    let content = registry_response_string(response, "content").unwrap_or_default();
    if content.trim().is_empty() {
        warnings
            .push("Mesh agent state is empty; generated an index with no artifacts.".to_string());
        return Ok(empty_agent_manifest(
            agent_name,
            agent_id,
            mesh_id,
            workspace_root,
        ));
    }

    match serde_json::from_str::<AgentPortableManifest>(&content) {
        Ok(mut manifest) => {
            manifest.agent_name = agent_name.to_string();
            manifest.agent_id = agent_id.to_string();
            manifest.mesh_id = Some(mesh_id.to_string());
            Ok(manifest)
        }
        Err(error) => {
            let format = registry_response_string(response, "format").unwrap_or_default();
            if format == "json" {
                warnings.push(format!(
                    "Mesh agent state was not a portable manifest ({error}); generated an index with no artifacts."
                ));
                Ok(empty_agent_manifest(
                    agent_name,
                    agent_id,
                    mesh_id,
                    workspace_root,
                ))
            } else {
                warnings
                    .push("Loaded legacy agent content as the portable index only.".to_string());
                let mut manifest =
                    empty_agent_manifest(agent_name, agent_id, mesh_id, workspace_root);
                manifest.index.content = content;
                manifest.index.sha256 = sha256_hex(manifest.index.content.as_bytes());
                Ok(manifest)
            }
        }
    }
}

fn manifest_from_artifacts(
    agent_name: &str,
    agent_id: &str,
    mesh_id: &str,
    workspace_root: &Path,
    artifacts: Vec<AgentManifestArtifact>,
) -> AgentPortableManifest {
    let generated_at = now_unix_seconds();
    let workspace = agent_workspace_metadata(workspace_root);
    let index_path = agent_index_path(agent_name);
    let index = AgentIndexArtifact {
        path: index_path,
        kind: "index".to_string(),
        format: "md".to_string(),
        content: String::new(),
        sha256: String::new(),
        generated_at_unix_seconds: generated_at,
    };

    AgentPortableManifest {
        schema_version: AGENT_PORTABLE_SCHEMA_VERSION,
        agent_name: agent_name.to_string(),
        agent_id: agent_id.to_string(),
        generated_at_unix_seconds: generated_at,
        mesh_id: Some(mesh_id.to_string()),
        workspace,
        index,
        artifacts,
    }
}

fn empty_agent_manifest(
    agent_name: &str,
    agent_id: &str,
    mesh_id: &str,
    workspace_root: &Path,
) -> AgentPortableManifest {
    manifest_from_artifacts(agent_name, agent_id, mesh_id, workspace_root, Vec::new())
}

fn ensure_agent_index(
    manifest: &mut AgentPortableManifest,
    agent_name: &str,
    agent_id: &str,
    mesh_id: &str,
    workspace_root: &Path,
) {
    if manifest.index.path.trim().is_empty() {
        manifest.index.path = agent_index_path(agent_name);
    }
    manifest.index.kind = "index".to_string();
    manifest.index.format = "md".to_string();
    if manifest.workspace.path.is_none() && manifest.workspace.host.is_none() {
        manifest.workspace = agent_workspace_metadata(workspace_root);
    }
    if manifest.index.generated_at_unix_seconds <= 0 {
        manifest.index.generated_at_unix_seconds = now_unix_seconds();
    }
    if manifest.index.content.trim().is_empty() {
        manifest.index.content = render_agent_index_content(
            agent_name,
            agent_id,
            mesh_id,
            manifest.generated_at_unix_seconds,
            &manifest.workspace,
            &manifest.artifacts,
        );
    }
    manifest.index.sha256 = sha256_hex(manifest.index.content.as_bytes());
}

fn render_agent_index_content(
    agent_name: &str,
    agent_id: &str,
    mesh_id: &str,
    generated_at: i64,
    workspace: &AgentWorkspaceMetadata,
    artifacts: &[AgentManifestArtifact],
) -> String {
    let mut lines = vec![
        format!("# {agent_name} Portable Agent Index"),
        String::new(),
        "This file is generated by `smesh agent save` and restored by `smesh agent init`."
            .to_string(),
        String::new(),
        format!("- Agent ID: `{agent_id}`"),
        format!("- Mesh ID: `{mesh_id}`"),
        format!("- Generated at Unix seconds: `{generated_at}`"),
    ];
    if let Some(path) = workspace.path.as_deref() {
        lines.push(format!("- Source workspace: `{path}`"));
    }
    if let Some(host) = workspace.host.as_deref() {
        lines.push(format!("- Source host: `{host}`"));
    }
    lines.push(String::new());
    lines.push("## Artifacts".to_string());
    if artifacts.is_empty() {
        lines.push(String::new());
        lines.push("No portable Markdown artifacts are stored for this agent yet.".to_string());
    } else {
        lines.push(String::new());
        for artifact in artifacts {
            lines.push(format!(
                "- `{}` ({}, sha256 `{}`)",
                artifact.path, artifact.kind, artifact.sha256
            ));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

fn discover_agent_markdown_artifacts(
    workspace_root: &Path,
) -> Result<Vec<AgentManifestArtifact>, CliError> {
    let mut discovered = BTreeMap::<PathBuf, String>::new();
    for (path, kind) in [
        ("SOUL.md", "identity"),
        ("AGENTS.md", "operating_doc"),
        ("CLAUDE.md", "operating_doc"),
        ("MEMORY.md", "memory"),
        ("USER.md", "user_profile"),
        ("TOOLS.md", "tools"),
    ] {
        let rel = PathBuf::from(path);
        if workspace_root.join(&rel).is_file() {
            discovered.insert(rel, kind.to_string());
        }
    }

    collect_markdown_under(workspace_root, Path::new(".codex/skills"), &mut discovered)?;
    collect_markdown_under(workspace_root, Path::new("skills"), &mut discovered)?;
    collect_markdown_under(workspace_root, Path::new("references"), &mut discovered)?;
    collect_markdown_under(workspace_root, Path::new("docs/agents"), &mut discovered)?;
    collect_markdown_under(workspace_root, Path::new("docs/skills"), &mut discovered)?;
    collect_markdown_under(workspace_root, Path::new(".claude"), &mut discovered)?;
    collect_markdown_under(workspace_root, Path::new(".cursor/rules"), &mut discovered)?;

    let now = now_unix_seconds();
    let source = AgentArtifactSource {
        workspace_path: Some(workspace_root.display().to_string()),
        host: host_identity(),
    };
    let mut artifacts = Vec::new();
    for (relative_path, kind) in discovered {
        let path = workspace_root.join(&relative_path);
        let content = fs::read_to_string(&path).map_err(|error| {
            CliError::config(format!(
                "Failed to read agent artifact {}: {error}",
                path.display()
            ))
        })?;
        let portable_path = portable_path_string(&relative_path)?;
        let bytes = content.len();
        artifacts.push(AgentManifestArtifact {
            path: portable_path,
            kind,
            format: "md".to_string(),
            sha256: sha256_hex(content.as_bytes()),
            size_bytes: bytes,
            captured_at_unix_seconds: now,
            source: source.clone(),
            content,
        });
    }

    Ok(artifacts)
}

fn collect_markdown_under(
    workspace_root: &Path,
    relative_dir: &Path,
    discovered: &mut BTreeMap<PathBuf, String>,
) -> Result<(), CliError> {
    let dir = workspace_root.join(relative_dir);
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir).map_err(|error| {
        CliError::config(format!(
            "Failed to scan agent artifact directory {}: {error}",
            dir.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            CliError::config(format!(
                "Failed to read agent artifact entry in {}: {error}",
                dir.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            CliError::config(format!(
                "Failed to inspect agent artifact {}: {error}",
                entry.path().display()
            ))
        })?;
        let relative_path = relative_dir.join(entry.file_name());
        if file_type.is_dir() {
            collect_markdown_under(workspace_root, &relative_path, discovered)?;
        } else if file_type.is_file() && is_markdown_path(&relative_path) {
            discovered
                .entry(relative_path.clone())
                .or_insert_with(|| artifact_kind_for_path(&relative_path));
        }
    }
    Ok(())
}

fn write_generated_agent_index(
    workspace_root: &Path,
    index: &AgentIndexArtifact,
) -> Result<(), CliError> {
    let path = resolve_safe_agent_path(workspace_root, &index.path)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            CliError::config(format!(
                "Failed to create directory for {}: {error}",
                path.display()
            ))
        })?;
    }
    ensure_safe_agent_write_target(workspace_root, &path, &index.path)?;
    fs::write(&path, index.content.as_bytes()).map_err(|error| {
        CliError::config(format!(
            "Failed to write generated agent index {}: {error}",
            path.display()
        ))
    })
}

fn write_agent_workspace_file(
    workspace_root: &Path,
    relative_path: &str,
    kind: &str,
    content: &str,
    override_existing: bool,
) -> Result<AgentFileOutput, CliError> {
    let path = resolve_safe_agent_path(workspace_root, relative_path)?;
    let existed = path.exists();
    let sha256 = sha256_hex(content.as_bytes());
    let bytes = content.len();

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            CliError::config(format!(
                "Failed to create directory for {}: {error}",
                path.display()
            ))
        })?;
    }
    ensure_safe_agent_write_target(workspace_root, &path, relative_path)?;

    if existed && !override_existing {
        return Ok(AgentFileOutput {
            path: relative_path.to_string(),
            kind: kind.to_string(),
            sha256,
            bytes,
            status: "skipped".to_string(),
            reason: Some("exists".to_string()),
        });
    }

    fs::write(&path, content.as_bytes()).map_err(|error| {
        CliError::config(format!(
            "Failed to write agent artifact {}: {error}",
            path.display()
        ))
    })?;

    Ok(AgentFileOutput {
        path: relative_path.to_string(),
        kind: kind.to_string(),
        sha256,
        bytes,
        status: if existed { "overwritten" } else { "restored" }.to_string(),
        reason: None,
    })
}

fn ensure_safe_agent_write_target(
    workspace_root: &Path,
    path: &Path,
    relative_path: &str,
) -> Result<(), CliError> {
    let workspace_root = workspace_root.canonicalize().map_err(|error| {
        CliError::config(format!(
            "Failed to resolve agent workspace root {}: {error}",
            workspace_root.display()
        ))
    })?;
    let parent = path.parent().ok_or_else(|| {
        CliError::config(format!(
            "Unsafe agent artifact path `{relative_path}`: missing parent directory."
        ))
    })?;
    let parent = parent.canonicalize().map_err(|error| {
        CliError::config(format!(
            "Failed to resolve parent directory for agent artifact `{relative_path}`: {error}"
        ))
    })?;
    if !parent.starts_with(&workspace_root) {
        return Err(CliError::config(format!(
            "Unsafe agent artifact path `{relative_path}`: resolved parent escapes the workspace."
        )));
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CliError::config(format!(
            "Unsafe agent artifact path `{relative_path}`: symlink targets are not allowed."
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::config(format!(
            "Failed to inspect agent artifact `{relative_path}` before writing: {error}"
        ))),
    }
}

fn resolve_safe_agent_path(
    workspace_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, CliError> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() {
        return Err(CliError::config("Unsafe agent artifact path: empty path."));
    }
    if trimmed.contains('\\') {
        return Err(CliError::config(format!(
            "Unsafe agent artifact path `{relative_path}`: backslashes are not portable."
        )));
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(CliError::config(format!(
            "Unsafe agent artifact path `{relative_path}`: absolute paths are not allowed."
        )));
    }

    let mut normalized = PathBuf::new();
    let mut first_component = true;
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                if first_component && part == OsStr::new(".git") {
                    return Err(CliError::config(format!(
                        "Unsafe agent artifact path `{relative_path}`: .git paths are not allowed."
                    )));
                }
                first_component = false;
                normalized.push(part);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(CliError::config(format!(
                    "Unsafe agent artifact path `{relative_path}`: parent traversal is not allowed."
                )));
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(CliError::config(format!(
                    "Unsafe agent artifact path `{relative_path}`: rooted paths are not allowed."
                )));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(CliError::config(format!(
            "Unsafe agent artifact path `{relative_path}`: empty normalized path."
        )));
    }
    Ok(workspace_root.join(normalized))
}

fn portable_path_string(path: &Path) -> Result<String, CliError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    CliError::config(format!(
                        "Agent artifact path {} is not valid UTF-8.",
                        path.display()
                    ))
                })?;
                parts.push(part.to_string());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(CliError::config(format!(
                    "Unsafe agent artifact path {}.",
                    path.display()
                )));
            }
        }
    }
    if parts.is_empty() {
        return Err(CliError::config("Agent artifact path is empty."));
    }
    Ok(parts.join("/"))
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

fn artifact_kind_for_path(path: &Path) -> String {
    let path_text = portable_path_string(path).unwrap_or_else(|_| path.display().to_string());
    if path_text.contains("/references/") || path_text.starts_with("references/") {
        "reference".to_string()
    } else if path_text.contains("/skills/")
        || path_text.starts_with("skills/")
        || path_text.starts_with(".codex/skills/")
    {
        "skill".to_string()
    } else if path_text.starts_with(".claude/") || path_text.starts_with(".cursor/rules/") {
        "operating_doc".to_string()
    } else {
        "reference".to_string()
    }
}

fn agent_index_path(agent_name: &str) -> String {
    format!(".agent-{}.md", agent_slug(agent_name))
}

fn agent_slug(agent_name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in agent_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "agent".to_string()
    } else {
        slug
    }
}

fn normalize_agent_name(name: &str) -> Result<String, CliError> {
    let normalized = name.trim();
    if normalized.is_empty() {
        return Err(CliError::config("agent name is required."));
    }
    Ok(normalized.to_string())
}

fn current_workspace_dir() -> Result<PathBuf, CliError> {
    std::env::current_dir()
        .map_err(|error| CliError::config(format!("Failed to resolve current workspace: {error}")))
}

fn agent_workspace_metadata(workspace_root: &Path) -> AgentWorkspaceMetadata {
    AgentWorkspaceMetadata {
        path: Some(workspace_root.display().to_string()),
        host: host_identity(),
    }
}

fn host_identity() -> Option<String> {
    for name in ["SMESH_HOST_ID", "HOSTNAME", "COMPUTERNAME"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn registry_response_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(scalar_string)
        .filter(|text| !text.is_empty())
}

fn sha256_hex(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn render_job_command(cli: &Cli, command: &JobCommands) -> Result<String, CliError> {
    match command {
        JobCommands::Get(args) => {
            let id = encode_path_segment(&args.id);
            render_remote_status(
                cli,
                "jobs.get",
                "Job",
                &format!("/api/cli/jobs/{id}"),
                &format!("/v1/jobs/{id}"),
            )
        }
    }
}

fn render_capture_command(cli: &Cli, command: &CaptureCommands) -> Result<String, CliError> {
    match command {
        CaptureCommands::Text(args) => render_capture_text(cli, args),
        CaptureCommands::File(args) => render_capture_file(cli, args),
        CaptureCommands::Status(args) => {
            let id = encode_path_segment(&args.id);
            render_remote_status(
                cli,
                "capture.status",
                "Capture",
                &format!("/api/cli/capture/status/{id}"),
                &format!("/v1/captures/{id}/status"),
            )
        }
    }
}

fn render_capture_text(cli: &Cli, args: &CaptureTextArgs) -> Result<String, CliError> {
    let context = remote_context(cli, true)?;
    let mesh_id = context.mesh_id.clone().ok_or_else(|| {
        CliError::config(
            "Mesh context is required for capture. Pass --mesh-id or set SMESH_MESH_ID.",
        )
    })?;
    let text = args.text.join(" ").trim().to_string();
    if text.is_empty() {
        return Err(CliError::config("capture text requires non-empty text."));
    }

    let tags = normalize_tags(&args.tags);
    let mut payload = Map::new();
    payload.insert("text".to_string(), Value::String(text));
    payload.insert("mesh_id".to_string(), Value::String(mesh_id.clone()));
    if let Some(instructions) = non_empty_string(args.instructions.as_deref()) {
        payload.insert(
            "instructions".to_string(),
            Value::String(instructions.to_string()),
        );
    }
    if !tags.is_empty() {
        payload.insert(
            "tags".to_string(),
            Value::Array(tags.iter().cloned().map(Value::String).collect()),
        );
    }

    let response = post_json_with_fallback(
        &context,
        "/api/cli/capture/text",
        "/v1/capture/text",
        Value::Object(payload),
    )?;
    let normalized = normalize_capture_response(
        response,
        CaptureResponseContext {
            capture_type: "text",
            mesh_id: &mesh_id,
            filename: None,
            content_type: None,
            tags: &tags,
        },
    );
    render_capture_enqueue_value(cli.effective_output(), "capture.text", normalized)
}

fn render_capture_file(cli: &Cli, args: &CaptureFileArgs) -> Result<String, CliError> {
    let context = remote_context(cli, true)?;
    let mesh_id = context.mesh_id.clone().ok_or_else(|| {
        CliError::config(
            "Mesh context is required for capture. Pass --mesh-id or set SMESH_MESH_ID.",
        )
    })?;
    let path = expand_tilde(&args.path);
    if !path.exists() {
        return Err(CliError::config(format!(
            "File not found: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(CliError::config(format!("Not a file: {}", path.display())));
    }

    let file_content = fs::read(&path).map_err(|error| {
        CliError::config(format!("Failed to read file {}: {error}", path.display()))
    })?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| CliError::config(format!("Invalid file path: {}", path.display())))?;
    let content_type = detect_mime_type(&path, args.mime_type.as_deref(), &file_content);
    let tags = normalize_tags(&args.tags);

    let mut fields = Vec::new();
    if let Some(instructions) = non_empty_string(args.instructions.as_deref()) {
        fields.push(("instructions".to_string(), instructions.to_string()));
    }
    if !tags.is_empty() {
        fields.push((
            "tags".to_string(),
            serde_json::to_string(&tags).map_err(CliError::Serialize)?,
        ));
    }

    let response = post_multipart_with_fallback(
        &context,
        "/api/cli/capture/file",
        "/v1/capture/file",
        MultipartUpload {
            file_field: "file",
            filename: &filename,
            content_type: &content_type,
            file_content: &file_content,
            fields: &fields,
        },
    )?;
    let normalized = normalize_capture_response(
        response,
        CaptureResponseContext {
            capture_type: "file",
            mesh_id: &mesh_id,
            filename: Some(&filename),
            content_type: Some(&content_type),
            tags: &tags,
        },
    );
    render_capture_enqueue_value(cli.effective_output(), "capture.file", normalized)
}

fn render_search_command(cli: &Cli, args: &SearchArgs) -> Result<String, CliError> {
    if args.top_k == 0 {
        return Err(CliError::config("--top-k must be at least 1."));
    }

    let query = joined_words(&args.query);
    if query.is_empty() {
        return Err(CliError::config("Search query cannot be empty."));
    }

    let filters = SearchOutputFilters {
        labels: collect_repeated_values(&args.filters),
        date_from: clean_optional(args.date_from.as_deref()),
        date_to: clean_optional(args.date_to.as_deref()),
    };

    let mut payload = serde_json::Map::new();
    payload.insert("query".to_string(), Value::String(query.clone()));
    payload.insert("top_k".to_string(), json!(args.top_k));

    let mut filter_payload = serde_json::Map::new();
    if !filters.labels.is_empty() {
        filter_payload.insert("labels".to_string(), json!(&filters.labels));
    }
    if let Some(date_from) = &filters.date_from {
        filter_payload.insert("date_from".to_string(), Value::String(date_from.clone()));
    }
    if let Some(date_to) = &filters.date_to {
        filter_payload.insert("date_to".to_string(), Value::String(date_to.clone()));
    }
    if !filter_payload.is_empty() {
        payload.insert("filters".to_string(), Value::Object(filter_payload));
    }

    let value = post_cli_json_with_fallback(
        cli,
        "/api/cli/search",
        &Value::Object(payload),
        "/v1/search",
        Duration::from_secs(30),
    )?;
    let output = normalize_search_output(query, args.top_k, filters, &value);
    render_retrieval_output(
        cli.effective_output(),
        "search.results",
        &output,
        render_search_human(&output),
    )
}

fn render_ask_command(cli: &Cli, args: &AskArgs) -> Result<String, CliError> {
    let question = joined_words(&args.question);
    if question.is_empty() {
        return Err(CliError::config("Question cannot be empty."));
    }

    let payload = json!({
        "question": question,
        "stream": false,
    });
    let value = post_cli_json_with_fallback(
        cli,
        "/api/cli/ask",
        &payload,
        "/v1/ask",
        Duration::from_secs(90),
    )?;
    let output = normalize_ask_output(
        payload
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        &value,
    );
    render_retrieval_output(
        cli.effective_output(),
        "ask.answer",
        &output,
        render_ask_human(&output),
    )
}

fn render_topic_command(cli: &Cli, command: &TopicCommands) -> Result<String, CliError> {
    match command {
        TopicCommands::Query(args) => render_topics_query_command(cli, args),
        TopicCommands::Activity(args) => render_topics_activity_command(cli, args),
    }
}

fn render_topics_query_command(cli: &Cli, args: &TopicQueryArgs) -> Result<String, CliError> {
    if args.limit == 0 {
        return Err(CliError::config("--limit must be at least 1."));
    }

    let topics = collect_repeated_values(&args.common.topics);
    if topics.is_empty() {
        return Err(CliError::config("At least one --topic is required."));
    }
    let exclude_node_types = collect_repeated_values(&args.common.exclude_node_types);
    let window = topic_window(&args.common);
    let sort = if args.recent {
        TopicSort::Recent
    } else {
        args.sort
    };

    let mut payload = serde_json::Map::new();
    payload.insert("topics".to_string(), json!(&topics));
    payload.insert(
        "match".to_string(),
        Value::String(args.common.match_mode.as_str().to_string()),
    );
    payload.insert("limit".to_string(), json!(args.limit));
    payload.insert("offset".to_string(), json!(args.offset));
    payload.insert("sort".to_string(), Value::String(sort.as_str().to_string()));
    if !exclude_node_types.is_empty() {
        payload.insert("exclude_node_types".to_string(), json!(&exclude_node_types));
    }
    if let Some(window_payload) = topic_window_payload(&window) {
        payload.insert("window".to_string(), window_payload);
    }

    let value = post_cli_json(
        cli,
        "/api/topics/query",
        &Value::Object(payload),
        Duration::from_secs(30),
    )?;
    let output = normalize_topic_query_output(
        topics,
        args.common.match_mode.as_str().to_string(),
        args.limit,
        args.offset,
        sort.as_str().to_string(),
        window,
        exclude_node_types,
        &value,
    );
    render_retrieval_output(
        cli.effective_output(),
        "topics.query",
        &output,
        render_topics_query_human(&output),
    )
}

fn render_topics_activity_command(cli: &Cli, args: &TopicActivityArgs) -> Result<String, CliError> {
    let topics = collect_repeated_values(&args.common.topics);
    if topics.is_empty() {
        return Err(CliError::config("At least one --topic is required."));
    }
    let exclude_node_types = collect_repeated_values(&args.common.exclude_node_types);
    let window = topic_window(&args.common);

    let mut payload = serde_json::Map::new();
    payload.insert("topics".to_string(), json!(&topics));
    payload.insert(
        "match".to_string(),
        Value::String(args.common.match_mode.as_str().to_string()),
    );
    if !exclude_node_types.is_empty() {
        payload.insert("exclude_node_types".to_string(), json!(&exclude_node_types));
    }
    if let Some(window_payload) = topic_window_payload(&window) {
        payload.insert("window".to_string(), window_payload);
    }

    let value = post_cli_json(
        cli,
        "/api/topics/activity",
        &Value::Object(payload),
        Duration::from_secs(30),
    )?;
    let output = normalize_topic_activity_output(
        topics,
        args.common.match_mode.as_str().to_string(),
        window,
        exclude_node_types,
        &value,
    );
    render_retrieval_output(
        cli.effective_output(),
        "topics.activity",
        &output,
        render_topics_activity_human(&output),
    )
}

fn render_project_command(cli: &Cli, command: &ProjectCommands) -> Result<String, CliError> {
    match command {
        ProjectCommands::Create(args) => {
            let mut payload = Map::new();
            payload.insert(
                "title".to_string(),
                Value::String(joined_words(&args.title)),
            );
            insert_optional_string(&mut payload, "summary", args.summary.as_deref());
            payload.insert("status".to_string(), Value::String(args.status.clone()));
            payload.insert(
                "visibility".to_string(),
                Value::String(args.visibility.clone()),
            );
            insert_string_array(&mut payload, "tags", normalize_tags(&args.tags));
            insert_optional_string(&mut payload, "source_type", args.source_type.as_deref());
            insert_optional_string(&mut payload, "source_id", args.source_id.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(cli, "/api/cli/projects", Value::Object(payload))?;
            render_assistant_value(cli.effective_output(), "projects.create", "Project", value)
        }
        ProjectCommands::List(args) => {
            if args.limit == 0 {
                return Err(CliError::config("--limit must be at least 1."));
            }
            let mut query = Vec::new();
            push_query_values(&mut query, "status", collect_repeated_values(&args.status));
            push_query_values(
                &mut query,
                "visibility",
                collect_repeated_values(&args.visibility),
            );
            push_query_opt(&mut query, "query", args.query.as_deref());
            if args.include_archived {
                push_query(&mut query, "include_archived", "true");
            }
            push_query(&mut query, "limit", args.limit.to_string());
            push_query(&mut query, "offset", args.offset.to_string());
            let value = get_assistant_json(cli, "/api/cli/projects", &query)?;
            render_assistant_value(cli.effective_output(), "projects.list", "Projects", value)
        }
        ProjectCommands::Get(args) => {
            let value = get_assistant_json(
                cli,
                &format!("/api/cli/projects/{}", encode_path_segment(&args.id)),
                &[],
            )?;
            render_assistant_value(cli.effective_output(), "projects.get", "Project", value)
        }
        ProjectCommands::Update(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "title", args.title.as_deref());
            insert_optional_string(&mut payload, "summary", args.summary.as_deref());
            insert_optional_string(&mut payload, "status", args.status.as_deref());
            insert_optional_string(&mut payload, "visibility", args.visibility.as_deref());
            insert_string_array(&mut payload, "tags", normalize_tags(&args.tags));
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = patch_assistant_json(
                cli,
                &format!("/api/cli/projects/{}", encode_path_segment(&args.id)),
                Value::Object(payload),
            )?;
            render_assistant_value(cli.effective_output(), "projects.update", "Project", value)
        }
        ProjectCommands::Archive(args) => {
            let mut payload = Map::new();
            payload.insert("confirm".to_string(), json!(args.confirm));
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/projects/{}/archive",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(cli.effective_output(), "projects.archive", "Project", value)
        }
    }
}

fn render_assertion_command(cli: &Cli, command: &AssertionCommands) -> Result<String, CliError> {
    match command {
        AssertionCommands::List(args) => {
            if args.limit == 0 {
                return Err(CliError::config("--limit must be at least 1."));
            }
            let mut query = Vec::new();
            push_query(&mut query, "kind", args.kind.clone());
            push_query_values(&mut query, "status", collect_repeated_values(&args.status));
            push_query_opt(&mut query, "project_id", args.project_id.as_deref());
            push_query(&mut query, "limit", args.limit.to_string());
            push_query(&mut query, "offset", args.offset.to_string());
            let value = get_assistant_json(cli, "/api/cli/assertions", &query)?;
            render_assistant_value(
                cli.effective_output(),
                "assertions.list",
                "Assertions",
                value,
            )
        }
        AssertionCommands::Get(args) => {
            let value = get_assistant_json(
                cli,
                &format!("/api/cli/assertions/{}", encode_path_segment(&args.id)),
                &[],
            )?;
            render_assistant_value(cli.effective_output(), "assertions.get", "Assertion", value)
        }
        AssertionCommands::Confirm(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "project_id", args.project_id.as_deref());
            insert_optional_string(&mut payload, "title", args.title.as_deref());
            insert_optional_string(&mut payload, "summary", args.summary.as_deref());
            insert_optional_string(&mut payload, "status", args.status.as_deref());
            insert_optional_string(&mut payload, "visibility", args.visibility.as_deref());
            insert_optional_string(&mut payload, "reason", args.reason.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/assertions/{}/confirm",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "assertions.confirm",
                "Assertion",
                value,
            )
        }
        AssertionCommands::Deny(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "reason", args.reason.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!("/api/cli/assertions/{}/deny", encode_path_segment(&args.id)),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "assertions.deny",
                "Assertion",
                value,
            )
        }
        AssertionCommands::Merge(args) | AssertionCommands::Attach(args) => {
            let mut payload = Map::new();
            payload.insert(
                "target_project_id".to_string(),
                Value::String(args.target_project_id.clone()),
            );
            payload.insert(
                "merge_mode".to_string(),
                Value::String(args.merge_mode.clone()),
            );
            insert_optional_string(&mut payload, "title", args.title.as_deref());
            insert_optional_string(&mut payload, "summary", args.summary.as_deref());
            insert_optional_string(&mut payload, "status", args.status.as_deref());
            insert_optional_string(&mut payload, "visibility", args.visibility.as_deref());
            insert_optional_string(&mut payload, "reason", args.reason.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let action = if matches!(command, AssertionCommands::Attach(_)) {
                "attach"
            } else {
                "merge"
            };
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/assertions/{}/{}",
                    encode_path_segment(&args.id),
                    action
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "assertions.merge",
                "Assertion",
                value,
            )
        }
        AssertionCommands::Delegate(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "to_agent", args.to_agent.as_deref());
            insert_optional_string(&mut payload, "to_user", args.to_user.as_deref());
            insert_optional_string(&mut payload, "note", args.note.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/assertions/{}/delegate",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "assertions.delegate",
                "Assertion",
                value,
            )
        }
    }
}

fn task_assertion_overrides_payload(
    args: &TaskAssertionTaskOverridesArgs,
    include_task_id: bool,
) -> Option<Value> {
    let mut payload = Map::new();
    if include_task_id {
        insert_optional_string(&mut payload, "task_id", args.task_id.as_deref());
    }
    insert_optional_string(&mut payload, "title", args.title.as_deref());
    insert_optional_string(&mut payload, "description", args.description.as_deref());
    insert_optional_string(&mut payload, "status", args.status.as_deref());
    insert_optional_string(&mut payload, "priority", args.priority.as_deref());
    insert_optional_string(&mut payload, "project_id", args.project_id.as_deref());
    insert_optional_string(&mut payload, "due_at", args.due_at.as_deref());
    insert_optional_string(&mut payload, "start_at", args.start_at.as_deref());
    insert_optional_string(&mut payload, "timezone", args.timezone.as_deref());
    insert_optional_string(&mut payload, "recurrence", args.recurrence.as_deref());
    insert_optional_string(
        &mut payload,
        "assignee_user_id",
        args.assignee_user_id.as_deref(),
    );
    insert_optional_string(&mut payload, "visibility", args.visibility.as_deref());
    insert_optional_string(&mut payload, "stale_at", args.stale_at.as_deref());
    insert_string_array(&mut payload, "tags", normalize_tags(&args.tags));
    if payload.is_empty() {
        None
    } else {
        Some(Value::Object(payload))
    }
}

fn read_task_assertion_bulk_review_payload(
    args: &TaskAssertionBulkReviewArgs,
) -> Result<Value, CliError> {
    if args.input.is_some() == args.stdin {
        return Err(CliError::config(
            "task-assertions bulk-review requires exactly one of --input or --stdin.",
        ));
    }

    let text = if let Some(path) = &args.input {
        let path = expand_tilde(path);
        fs::read_to_string(&path).map_err(|error| {
            CliError::config(format!("Failed to read {}: {error}", path.display()))
        })?
    } else {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| CliError::config(format!("Failed to read stdin: {error}")))?;
        text
    };

    let parsed: Value = serde_json::from_str(&text)
        .map_err(|error| CliError::config(format!("Invalid JSON for bulk review: {error}")))?;

    match parsed {
        Value::Array(operations) => Ok(json!({ "operations": operations })),
        Value::Object(map) if map.get("operations").is_some() => Ok(Value::Object(map)),
        _ => Err(CliError::config(
            "Bulk review input must be a JSON array or an object with an operations field.",
        )),
    }
}

fn render_task_assertion_command(
    cli: &Cli,
    command: &TaskAssertionCommands,
) -> Result<String, CliError> {
    match command {
        TaskAssertionCommands::List(args) => {
            if args.limit == 0 {
                return Err(CliError::config("--limit must be at least 1."));
            }
            let mut query = Vec::new();
            push_query_values(&mut query, "status", collect_repeated_values(&args.status));
            push_query_opt(&mut query, "asserted_by", args.asserted_by.as_deref());
            if let Some(confidence_gte) = args.confidence_gte {
                push_query(&mut query, "confidence_gte", confidence_gte.to_string());
            }
            push_query_opt(&mut query, "project_id", args.project_id.as_deref());
            push_query_opt(
                &mut query,
                "project_assertion_id",
                args.project_assertion_id.as_deref(),
            );
            push_query_opt(&mut query, "source_type", args.source_type.as_deref());
            push_query_opt(&mut query, "due_before", args.due_before.as_deref());
            push_query(&mut query, "limit", args.limit.to_string());
            push_query(&mut query, "offset", args.offset.to_string());
            let value = get_assistant_json(cli, "/api/cli/task-assertions", &query)?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.list",
                "Assertions",
                value,
            )
        }
        TaskAssertionCommands::Get(args) => {
            let value = get_assistant_json(
                cli,
                &format!("/api/cli/task-assertions/{}", encode_path_segment(&args.id)),
                &[],
            )?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.get",
                "Assertion",
                value,
            )
        }
        TaskAssertionCommands::Confirm(args) => {
            let mut payload = Map::new();
            if let Some(overrides) = task_assertion_overrides_payload(&args.overrides, true) {
                payload.insert("promotion_overrides".to_string(), overrides);
            }
            insert_optional_string(&mut payload, "reason", args.reason.as_deref());
            insert_optional_string(&mut payload, "note", args.note.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/task-assertions/{}/confirm",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.confirm",
                "Assertion",
                value,
            )
        }
        TaskAssertionCommands::Deny(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "reason", args.reason.as_deref());
            insert_optional_string(&mut payload, "note", args.note.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/task-assertions/{}/deny",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.deny",
                "Assertion",
                value,
            )
        }
        TaskAssertionCommands::Merge(args) => {
            let mut payload = Map::new();
            payload.insert("task_id".to_string(), Value::String(args.task_id.clone()));
            if let Some(overrides) = task_assertion_overrides_payload(&args.overrides, false) {
                payload.insert("merge_overrides".to_string(), overrides);
            }
            insert_optional_string(&mut payload, "reason", args.reason.as_deref());
            insert_optional_string(&mut payload, "note", args.note.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/task-assertions/{}/merge",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.merge",
                "Assertion",
                value,
            )
        }
        TaskAssertionCommands::Delegate(args) => {
            if args.to_agent.is_none() && args.to_user.is_none() {
                return Err(CliError::config(
                    "task-assertions delegate requires --to-agent or --to-user.",
                ));
            }
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "to_agent", args.to_agent.as_deref());
            insert_optional_string(&mut payload, "to_user", args.to_user.as_deref());
            insert_optional_string(&mut payload, "note", args.note.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/task-assertions/{}/delegate",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.delegate",
                "Assertion",
                value,
            )
        }
        TaskAssertionCommands::AttachProject(args) => {
            if args.clear && (args.project_id.is_some() || args.project_assertion_id.is_some()) {
                return Err(CliError::config(
                    "task-assertions attach-project does not allow --clear with project flags.",
                ));
            }
            if !args.clear && args.project_id.is_none() && args.project_assertion_id.is_none() {
                return Err(CliError::config(
                    "task-assertions attach-project requires --project-id, --project-assertion-id, or --clear.",
                ));
            }
            let mut payload = Map::new();
            if args.clear {
                payload.insert("clear".to_string(), Value::Bool(true));
            }
            insert_optional_string(&mut payload, "project_id", args.project_id.as_deref());
            insert_optional_string(
                &mut payload,
                "project_assertion_id",
                args.project_assertion_id.as_deref(),
            );
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/task-assertions/{}/attach-project",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.attach_project",
                "Assertion",
                value,
            )
        }
        TaskAssertionCommands::BulkReview(args) => {
            let mut payload = read_task_assertion_bulk_review_payload(args)?;
            let map = payload
                .as_object_mut()
                .ok_or_else(|| CliError::config("Bulk review payload must be a JSON object."))?;
            insert_agent_write_fields(map, &args.write)?;
            let value = post_assistant_json(cli, "/api/cli/task-assertions/bulk-review", payload)?;
            render_assistant_value(
                cli.effective_output(),
                "task_assertions.bulk_review",
                "Assertions",
                value,
            )
        }
    }
}

fn render_task_command(cli: &Cli, command: &TaskCommands) -> Result<String, CliError> {
    match command {
        TaskCommands::Create(args) => {
            let mut payload = Map::new();
            payload.insert(
                "title".to_string(),
                Value::String(joined_words(&args.title)),
            );
            insert_optional_string(&mut payload, "description", args.description.as_deref());
            payload.insert("status".to_string(), Value::String(args.status.clone()));
            payload.insert("priority".to_string(), Value::String(args.priority.clone()));
            insert_optional_string(&mut payload, "project_id", args.project_id.as_deref());
            insert_optional_string(&mut payload, "due_at", args.due_at.as_deref());
            insert_optional_string(&mut payload, "start_at", args.start_at.as_deref());
            insert_optional_string(&mut payload, "timezone", args.timezone.as_deref());
            insert_optional_string(&mut payload, "recurrence", args.recurrence.as_deref());
            insert_optional_string(
                &mut payload,
                "assignee_user_id",
                args.assignee_user_id.as_deref(),
            );
            payload.insert(
                "visibility".to_string(),
                Value::String(args.visibility.clone()),
            );
            insert_optional_string(&mut payload, "stale_at", args.stale_at.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            insert_string_array(&mut payload, "tags", normalize_tags(&args.tags));
            insert_optional_string(&mut payload, "source_type", args.source_type.as_deref());
            insert_optional_string(&mut payload, "source_id", args.source_id.as_deref());
            let value = post_assistant_json(cli, "/api/cli/tasks", Value::Object(payload))?;
            render_assistant_value(cli.effective_output(), "tasks.create", "Task", value)
        }
        TaskCommands::List(args) => {
            if args.limit == 0 {
                return Err(CliError::config("--limit must be at least 1."));
            }
            let mut query = Vec::new();
            push_query_values(&mut query, "status", collect_repeated_values(&args.status));
            push_query_opt(&mut query, "due_before", args.due_before.as_deref());
            push_query_opt(&mut query, "due_after", args.due_after.as_deref());
            push_query_opt(&mut query, "stale_before", args.stale_before.as_deref());
            push_query_opt(&mut query, "project_id", args.project_id.as_deref());
            push_query_opt(
                &mut query,
                "assignee_user_id",
                args.assignee_user_id.as_deref(),
            );
            push_query_values(&mut query, "tags", normalize_tags(&args.tags));
            push_query(&mut query, "limit", args.limit.to_string());
            push_query(&mut query, "offset", args.offset.to_string());
            let value = get_assistant_json(cli, "/api/cli/tasks", &query)?;
            render_assistant_value(cli.effective_output(), "tasks.list", "Tasks", value)
        }
        TaskCommands::Get(args) => {
            let value = get_assistant_json(
                cli,
                &format!("/api/cli/tasks/{}", encode_path_segment(&args.id)),
                &[],
            )?;
            render_assistant_value(cli.effective_output(), "tasks.get", "Task", value)
        }
        TaskCommands::Update(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "title", args.title.as_deref());
            insert_optional_string(&mut payload, "description", args.description.as_deref());
            insert_optional_string(&mut payload, "status", args.status.as_deref());
            insert_optional_string(&mut payload, "priority", args.priority.as_deref());
            insert_optional_string(&mut payload, "project_id", args.project_id.as_deref());
            insert_optional_string(&mut payload, "due_at", args.due_at.as_deref());
            insert_optional_string(&mut payload, "start_at", args.start_at.as_deref());
            insert_optional_string(&mut payload, "timezone", args.timezone.as_deref());
            insert_optional_string(&mut payload, "recurrence", args.recurrence.as_deref());
            insert_optional_string(
                &mut payload,
                "assignee_user_id",
                args.assignee_user_id.as_deref(),
            );
            insert_optional_string(&mut payload, "stale_at", args.stale_at.as_deref());
            insert_string_array(&mut payload, "tags", normalize_tags(&args.tags));
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = patch_assistant_json(
                cli,
                &format!("/api/cli/tasks/{}", encode_path_segment(&args.id)),
                Value::Object(payload),
            )?;
            render_assistant_value(cli.effective_output(), "tasks.update", "Task", value)
        }
        TaskCommands::Complete(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "note", args.note.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!("/api/cli/tasks/{}/complete", encode_path_segment(&args.id)),
                Value::Object(payload),
            )?;
            render_assistant_value(cli.effective_output(), "tasks.complete", "Task", value)
        }
        TaskCommands::Delegate(args) => {
            if args.to_agent.is_none() && args.to_user.is_none() && args.to_contact.is_none() {
                return Err(CliError::config(
                    "tasks delegate requires --to-agent, --to-user, or --to-contact.",
                ));
            }
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "to_agent", args.to_agent.as_deref());
            insert_optional_string(&mut payload, "to_user", args.to_user.as_deref());
            insert_optional_string(&mut payload, "to_contact", args.to_contact.as_deref());
            insert_optional_string(&mut payload, "note", args.note.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!("/api/cli/tasks/{}/delegate", encode_path_segment(&args.id)),
                Value::Object(payload),
            )?;
            render_assistant_value(cli.effective_output(), "tasks.delegate", "Task", value)
        }
        TaskCommands::AttachSource(args) => {
            let mut payload = Map::new();
            payload.insert(
                "source_type".to_string(),
                Value::String(args.source_type.clone()),
            );
            payload.insert(
                "source_id".to_string(),
                Value::String(args.source_id.clone()),
            );
            if let Some(span_start) = args.span_start {
                payload.insert("span_start".to_string(), json!(span_start));
            }
            if let Some(span_end) = args.span_end {
                payload.insert("span_end".to_string(), json!(span_end));
            }
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!("/api/cli/tasks/{}/sources", encode_path_segment(&args.id)),
                Value::Object(payload),
            )?;
            render_assistant_value(cli.effective_output(), "tasks.attach_source", "Task", value)
        }
    }
}

fn render_reminder_command(cli: &Cli, command: &ReminderCommands) -> Result<String, CliError> {
    match command {
        ReminderCommands::Create(args) => {
            let mut payload = Map::new();
            payload.insert(
                "title".to_string(),
                Value::String(joined_words(&args.title)),
            );
            insert_optional_string(&mut payload, "task_id", args.task_id.as_deref());
            payload.insert("strategy".to_string(), Value::String(args.strategy.clone()));
            insert_optional_string(&mut payload, "absolute_at", args.absolute_at.as_deref());
            insert_optional_string(&mut payload, "relative_to", args.relative_to.as_deref());
            insert_optional_string(&mut payload, "offset", args.offset.as_deref());
            insert_optional_string(&mut payload, "schedule", args.schedule.as_deref());
            payload.insert("channel".to_string(), Value::String(args.channel.clone()));
            let value = post_assistant_json(cli, "/api/cli/reminders", Value::Object(payload))?;
            render_assistant_value(
                cli.effective_output(),
                "reminders.create",
                "Reminder",
                value,
            )
        }
        ReminderCommands::List(args) => {
            let mut query = Vec::new();
            push_query_opt(&mut query, "task_id", args.task_id.as_deref());
            push_query_opt(&mut query, "due_before", args.due_before.as_deref());
            push_query_opt(&mut query, "due_after", args.due_after.as_deref());
            push_query_values(&mut query, "state", collect_repeated_values(&args.state));
            push_query(&mut query, "limit", args.limit.to_string());
            push_query(&mut query, "offset", args.offset.to_string());
            let value = get_assistant_json(cli, "/api/cli/reminders", &query)?;
            render_assistant_value(cli.effective_output(), "reminders.list", "Reminders", value)
        }
        ReminderCommands::DueSoon(args) => {
            let mut query = Vec::new();
            push_query(&mut query, "window", args.window.clone());
            push_query(&mut query, "limit", args.limit.to_string());
            let value = get_assistant_json(cli, "/api/cli/reminders/due-soon", &query)?;
            render_assistant_value(
                cli.effective_output(),
                "reminders.due_soon",
                "Reminders",
                value,
            )
        }
        ReminderCommands::Snooze(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "until", args.until.as_deref());
            insert_optional_string(&mut payload, "duration", args.duration.as_deref());
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/reminders/{}/snooze",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "reminders.snooze",
                "Reminder",
                value,
            )
        }
        ReminderCommands::Complete(args) => {
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/reminders/{}/complete",
                    encode_path_segment(&args.id)
                ),
                json!({ "complete_task": args.complete_task }),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "reminders.complete",
                "Reminder",
                value,
            )
        }
        ReminderCommands::Dismiss(args) => {
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/reminders/{}/dismiss",
                    encode_path_segment(&args.id)
                ),
                json!({}),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "reminders.dismiss",
                "Reminder",
                value,
            )
        }
    }
}

fn render_contact_command(cli: &Cli, command: &ContactCommands) -> Result<String, CliError> {
    match command {
        ContactCommands::People { command } => match command {
            ContactPeopleCommands::Create(args) => {
                let mut payload = Map::new();
                payload.insert("display_name".to_string(), Value::String(args.name.clone()));
                insert_optional_string(&mut payload, "org", args.org.as_deref());
                insert_optional_string(&mut payload, "email", args.email.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value =
                    post_assistant_json(cli, "/api/cli/contacts/people", Value::Object(payload))?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.people.create",
                    "Person",
                    value,
                )
            }
            ContactPeopleCommands::List(args) => {
                let mut query = Vec::new();
                push_query_opt(&mut query, "query", args.query.as_deref());
                if args.open_loops {
                    push_query(&mut query, "open_loops", "true");
                }
                push_query(&mut query, "limit", args.limit.to_string());
                push_query(&mut query, "offset", args.offset.to_string());
                let value = get_assistant_json(cli, "/api/cli/contacts/people", &query)?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.people.list",
                    "People",
                    value,
                )
            }
            ContactPeopleCommands::Get(args) => {
                let mut query = Vec::new();
                push_query_opt(&mut query, "include", args.include.as_deref());
                let value = get_assistant_json(
                    cli,
                    &format!("/api/cli/contacts/people/{}", encode_path_segment(&args.id)),
                    &query,
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.people.get",
                    "Person",
                    value,
                )
            }
            ContactPeopleCommands::Update(args) => {
                let mut payload = Map::new();
                insert_optional_string(&mut payload, "name", args.name.as_deref());
                insert_optional_string(&mut payload, "email", args.email.as_deref());
                insert_optional_string(&mut payload, "org", args.org.as_deref());
                insert_optional_string(&mut payload, "role", args.role.as_deref());
                insert_optional_string(&mut payload, "timezone", args.timezone.as_deref());
                insert_optional_string(
                    &mut payload,
                    "preferred_channel",
                    args.preferred_channel.as_deref(),
                );
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    &format!("/api/cli/contacts/people/{}", encode_path_segment(&args.id)),
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.people.update",
                    "Person",
                    value,
                )
            }
            ContactPeopleCommands::Archive(args) => {
                let mut payload = Map::new();
                insert_optional_string(&mut payload, "reason", args.reason.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    &format!(
                        "/api/cli/contacts/people/{}/archive",
                        encode_path_segment(&args.id)
                    ),
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.people.archive",
                    "Person",
                    value,
                )
            }
            ContactPeopleCommands::Merge(args) => {
                let mut payload = Map::new();
                payload.insert("into".to_string(), Value::String(args.into.clone()));
                insert_optional_string(&mut payload, "reason", args.reason.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    &format!(
                        "/api/cli/contacts/people/{}/merge",
                        encode_path_segment(&args.id)
                    ),
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.people.merge",
                    "Person",
                    value,
                )
            }
            ContactPeopleCommands::Note { command } => match command {
                ContactNoteCommands::Add(args) => {
                    let mut payload = Map::new();
                    payload.insert("body".to_string(), Value::String(args.body.clone()));
                    if let Some(source_id) = args.source_id.as_deref() {
                        payload.insert(
                            "source_refs".to_string(),
                            Value::Array(vec![json!({"type": "source_node", "id": source_id})]),
                        );
                    }
                    insert_agent_write_fields(&mut payload, &args.write)?;
                    let value = post_assistant_json(
                        cli,
                        &format!("/api/cli/contacts/{}/notes", encode_path_segment(&args.id)),
                        Value::Object(payload),
                    )?;
                    render_assistant_value(
                        cli.effective_output(),
                        "contacts.note.add",
                        "Contact",
                        value,
                    )
                }
            },
        },
        ContactCommands::Orgs { command } => match command {
            ContactOrgCommands::Create(args) => {
                let mut payload = Map::new();
                payload.insert("name".to_string(), Value::String(args.name.clone()));
                insert_optional_string(&mut payload, "domain", args.domain.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value =
                    post_assistant_json(cli, "/api/cli/contacts/orgs", Value::Object(payload))?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.orgs.create",
                    "Organization",
                    value,
                )
            }
            ContactOrgCommands::List(args) => {
                let mut query = Vec::new();
                push_query_opt(&mut query, "query", args.query.as_deref());
                push_query(&mut query, "limit", args.limit.to_string());
                push_query(&mut query, "offset", args.offset.to_string());
                let value = get_assistant_json(cli, "/api/cli/contacts/orgs", &query)?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.orgs.list",
                    "Organizations",
                    value,
                )
            }
            ContactOrgCommands::Get(args) => {
                let value = get_assistant_json(
                    cli,
                    &format!("/api/cli/contacts/orgs/{}", encode_path_segment(&args.id)),
                    &[],
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.orgs.get",
                    "Organization",
                    value,
                )
            }
            ContactOrgCommands::Update(args) => {
                let mut payload = Map::new();
                insert_optional_string(&mut payload, "name", args.name.as_deref());
                insert_optional_string(&mut payload, "domain", args.domain.as_deref());
                insert_optional_string(&mut payload, "website", args.website.as_deref());
                insert_optional_string(&mut payload, "summary", args.summary.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    &format!("/api/cli/contacts/orgs/{}", encode_path_segment(&args.id)),
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.orgs.update",
                    "Organization",
                    value,
                )
            }
            ContactOrgCommands::Archive(args) => {
                let mut payload = Map::new();
                insert_optional_string(&mut payload, "reason", args.reason.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    &format!(
                        "/api/cli/contacts/orgs/{}/archive",
                        encode_path_segment(&args.id)
                    ),
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.orgs.archive",
                    "Organization",
                    value,
                )
            }
            ContactOrgCommands::Merge(args) => {
                let mut payload = Map::new();
                payload.insert("into".to_string(), Value::String(args.into.clone()));
                insert_optional_string(&mut payload, "reason", args.reason.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    &format!(
                        "/api/cli/contacts/orgs/{}/merge",
                        encode_path_segment(&args.id)
                    ),
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.orgs.merge",
                    "Organization",
                    value,
                )
            }
        },
        ContactCommands::Links { command } => match command {
            ContactLinkCommands::Add(args) => {
                let value = post_assistant_json(
                    cli,
                    &format!("/api/cli/contacts/{}/links", encode_path_segment(&args.id)),
                    {
                        let mut payload = Map::new();
                        payload.insert(
                            "source_type".to_string(),
                            Value::String(args.source_type.clone()),
                        );
                        payload.insert(
                            "source_id".to_string(),
                            Value::String(args.source_id.clone()),
                        );
                        insert_agent_write_fields(&mut payload, &args.write)?;
                        Value::Object(payload)
                    },
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.links.add",
                    "Contact",
                    value,
                )
            }
        },
        ContactCommands::Relationships { command } => match command {
            ContactRelationshipCommands::Add(args) => {
                let mut payload = Map::new();
                payload.insert(
                    "from_contact_id".to_string(),
                    Value::String(args.from_contact_id.clone()),
                );
                payload.insert("to".to_string(), Value::String(args.to.clone()));
                payload.insert(
                    "relationship_type".to_string(),
                    Value::String(args.relationship_type.clone()),
                );
                insert_optional_string(&mut payload, "source_id", args.source_id.as_deref());
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    "/api/cli/contacts/relationships",
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.relationships.add",
                    "Relationship",
                    value,
                )
            }
            ContactRelationshipCommands::List(args) => {
                let value = get_assistant_json(
                    cli,
                    &format!(
                        "/api/cli/contacts/{}/relationships",
                        encode_path_segment(&args.id)
                    ),
                    &[],
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.relationships.list",
                    "Relationships",
                    value,
                )
            }
            ContactRelationshipCommands::Remove(args) => {
                let mut payload = Map::new();
                insert_agent_write_fields(&mut payload, &args.write)?;
                let value = post_assistant_json(
                    cli,
                    &format!(
                        "/api/cli/contacts/relationships/{}/remove",
                        encode_path_segment(&args.id)
                    ),
                    Value::Object(payload),
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.relationships.remove",
                    "Relationship",
                    value,
                )
            }
        },
        ContactCommands::OpenLoops { command } => match command {
            ContactOpenLoopCommands::List(args) => {
                let mut query = Vec::new();
                push_query_opt(&mut query, "person", args.person.as_deref());
                push_query_opt(&mut query, "org", args.org.as_deref());
                let value = get_assistant_json(cli, "/api/cli/contacts/open-loops", &query)?;
                render_assistant_value(
                    cli.effective_output(),
                    "contacts.open_loops.list",
                    "Open loops",
                    value,
                )
            }
        },
    }
}

fn render_preference_command(cli: &Cli, command: &PreferenceCommands) -> Result<String, CliError> {
    match command {
        PreferenceCommands::Set(args) => {
            let source_refs = match (args.source_type.as_deref(), args.source_id.as_deref()) {
                (Some(source_type), Some(source_id)) => {
                    vec![json!({ "type": source_type, "id": source_id })]
                }
                _ => Vec::new(),
            };
            let mut payload = Map::new();
            payload.insert("scope".to_string(), parse_scope_arg(&args.scope));
            payload.insert("key".to_string(), Value::String(args.key.clone()));
            payload.insert("value".to_string(), parse_preference_value(args)?);
            payload.insert(
                "value_type".to_string(),
                Value::String(args.value_type.clone()),
            );
            payload.insert("polarity".to_string(), Value::String(args.polarity.clone()));
            insert_optional_string(&mut payload, "enforcement", args.enforcement.as_deref());
            insert_optional_string(&mut payload, "sensitivity", args.sensitivity.as_deref());
            let surfaces = collect_repeated_values(&args.surface);
            if !surfaces.is_empty() {
                payload.insert("surface".to_string(), json!(surfaces));
            }
            let tools = collect_repeated_values(&args.applies_to_tools);
            if !tools.is_empty() {
                payload.insert("applies_to_tools".to_string(), json!(tools));
            }
            insert_optional_string(&mut payload, "status", args.status.as_deref());
            insert_optional_string(
                &mut payload,
                "confirmation_state",
                args.confirmation_state.as_deref(),
            );
            payload.insert("confidence".to_string(), json!(args.confidence));
            payload.insert("source_refs".to_string(), json!(source_refs));
            payload.insert(
                "update_rule".to_string(),
                Value::String(args.update_rule.clone()),
            );
            insert_optional_string(&mut payload, "stale_at", args.stale_at.as_deref());
            let conflicts_with = collect_repeated_values(&args.conflicts_with);
            if !conflicts_with.is_empty() {
                payload.insert("conflicts_with".to_string(), json!(conflicts_with));
            }
            let supersedes = collect_repeated_values(&args.supersedes);
            if !supersedes.is_empty() {
                payload.insert("supersedes".to_string(), json!(supersedes));
            }
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(cli, "/api/cli/preferences", Value::Object(payload))?;
            render_assistant_value(
                cli.effective_output(),
                "preferences.set",
                "Preference",
                value,
            )
        }
        PreferenceCommands::List(args) => {
            let mut query = Vec::new();
            push_query_opt(&mut query, "scope", args.scope.as_deref());
            push_query_opt(&mut query, "domain", args.domain.as_deref());
            push_query_opt(&mut query, "surface", args.surface.as_deref());
            push_query_opt(&mut query, "tool", args.tool.as_deref());
            push_query_values(&mut query, "status", collect_repeated_values(&args.status));
            push_query(
                &mut query,
                "min_confidence",
                args.min_confidence.to_string(),
            );
            push_query(&mut query, "limit", args.limit.to_string());
            let value = get_assistant_json(cli, "/api/cli/preferences", &query)?;
            render_assistant_value(
                cli.effective_output(),
                "preferences.list",
                "Preferences",
                value,
            )
        }
        PreferenceCommands::Resolve(args) => {
            let mut payload = Map::new();
            payload.insert("actor".to_string(), parse_actor_arg(&args.actor)?);
            payload.insert("surface".to_string(), Value::String(args.surface.clone()));
            let tools = collect_repeated_values(&args.tools);
            if !tools.is_empty() {
                payload.insert("tools".to_string(), json!(tools));
            }
            insert_optional_string(&mut payload, "action", args.action.as_deref());
            let targets = args
                .targets
                .iter()
                .map(|target| parse_target_arg(target))
                .collect::<Result<Vec<_>, _>>()?;
            if !targets.is_empty() {
                payload.insert("targets".to_string(), Value::Array(targets));
            }
            let instruction_refs = collect_repeated_values(&args.current_instruction_refs);
            if !instruction_refs.is_empty() {
                payload.insert(
                    "current_instruction_refs".to_string(),
                    json!(instruction_refs),
                );
            }
            payload.insert("include_evidence".to_string(), json!(args.include_evidence));
            let value =
                post_assistant_json(cli, "/api/cli/preferences/resolve", Value::Object(payload))?;
            render_assistant_value(
                cli.effective_output(),
                "preferences.resolve",
                "Resolved preferences",
                value,
            )
        }
        PreferenceCommands::Get(args) => {
            let value = get_assistant_json(
                cli,
                &format!("/api/cli/preferences/{}", encode_path_segment(&args.id)),
                &[],
            )?;
            render_assistant_value(
                cli.effective_output(),
                "preferences.get",
                "Preference",
                value,
            )
        }
        PreferenceCommands::Confirm(args) => {
            let mut payload = Map::new();
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/preferences/{}/confirm",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "preferences.confirm",
                "Preference",
                value,
            )
        }
        PreferenceCommands::Revoke(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "reason", args.reason.as_deref());
            payload.insert("confirm".to_string(), json!(args.confirm));
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/preferences/{}/revoke",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "preferences.revoke",
                "Preference",
                value,
            )
        }
        PreferenceCommands::Evidence(args) => {
            let value = get_assistant_json(
                cli,
                &format!(
                    "/api/cli/preferences/{}/evidence",
                    encode_path_segment(&args.id)
                ),
                &[],
            )?;
            render_assistant_value(
                cli.effective_output(),
                "preferences.evidence",
                "Preference evidence",
                value,
            )
        }
    }
}

fn render_brief_command(cli: &Cli, command: &BriefCommands) -> Result<String, CliError> {
    match command {
        BriefCommands::Daily(args) => {
            let mut payload = Map::new();
            insert_optional_string(&mut payload, "date", args.date.as_deref());
            payload.insert("refresh".to_string(), json!(args.refresh));
            let value = post_assistant_json(cli, "/api/cli/briefs/daily", Value::Object(payload))?;
            render_assistant_value(cli.effective_output(), "briefs.daily", "Brief", value)
        }
        BriefCommands::Project(args) => {
            let value = post_assistant_json(
                cli,
                "/api/cli/briefs/project",
                json!({
                    "project_id": args.project_id,
                    "since": args.since,
                    "refresh": args.refresh,
                }),
            )?;
            render_assistant_value(cli.effective_output(), "briefs.project", "Brief", value)
        }
        BriefCommands::Contact(args) => {
            let value = post_assistant_json(
                cli,
                "/api/cli/briefs/contact",
                json!({"contact_id": args.contact_id, "refresh": args.refresh}),
            )?;
            render_assistant_value(cli.effective_output(), "briefs.contact", "Brief", value)
        }
        BriefCommands::MeetingPrep(args) => {
            let value = post_assistant_json(
                cli,
                "/api/cli/briefs/meeting-prep",
                json!({"event_id": args.event_id, "refresh": args.refresh}),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "briefs.meeting_prep",
                "Brief",
                value,
            )
        }
        BriefCommands::WhatChanged(args) => {
            let value = post_assistant_json(
                cli,
                "/api/cli/briefs/what-changed",
                json!({"since": args.since, "scope": args.scope}),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "briefs.what_changed",
                "Brief",
                value,
            )
        }
    }
}

fn render_calendar_command(cli: &Cli, command: &CalendarCommands) -> Result<String, CliError> {
    match command {
        CalendarCommands::Events { command } => match command {
            CalendarEventCommands::List(args) => {
                let mut query = Vec::new();
                push_query(&mut query, "from", args.from_ts.clone());
                push_query(&mut query, "to", args.to_ts.clone());
                if args.include_attendees {
                    push_query(&mut query, "include_attendees", "true");
                }
                push_query(&mut query, "limit", args.limit.to_string());
                let value = get_assistant_json(cli, "/api/cli/calendar/events", &query)?;
                render_assistant_value(
                    cli.effective_output(),
                    "calendar.events.list",
                    "Events",
                    value,
                )
            }
            CalendarEventCommands::Get(args) => {
                let value = get_assistant_json(
                    cli,
                    &format!("/api/cli/calendar/events/{}", encode_path_segment(&args.id)),
                    &[],
                )?;
                render_assistant_value(
                    cli.effective_output(),
                    "calendar.events.get",
                    "Event",
                    value,
                )
            }
            CalendarEventCommands::Upsert(args) => {
                let path = expand_tilde(&args.file);
                let text = fs::read_to_string(&path).map_err(|error| {
                    CliError::config(format!("Failed to read {}: {error}", path.display()))
                })?;
                let parsed: Value = serde_json::from_str(&text).map_err(|error| {
                    CliError::config(format!("Invalid JSON in {}: {error}", path.display()))
                })?;
                let payload = if parsed.get("events").is_some() {
                    parsed
                } else if parsed.is_array() {
                    json!({ "events": parsed })
                } else {
                    json!({ "events": [parsed] })
                };
                let value = put_assistant_json(cli, "/api/cli/calendar/events", payload)?;
                render_assistant_value(
                    cli.effective_output(),
                    "calendar.events.upsert",
                    "Events",
                    value,
                )
            }
        },
        CalendarCommands::MeetingPrep(args) => {
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/calendar/meeting-prep/{}",
                    encode_path_segment(&args.event_id)
                ),
                json!({"refresh": args.refresh}),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "calendar.meeting_prep",
                "Brief",
                value,
            )
        }
    }
}

fn render_source_link_command(cli: &Cli, command: &SourceLinkCommands) -> Result<String, CliError> {
    match command {
        SourceLinkCommands::Add(args) => {
            let mut payload = Map::new();
            payload.insert(
                "target_type".to_string(),
                Value::String(args.target_type.clone()),
            );
            payload.insert(
                "target_id".to_string(),
                Value::String(args.target_id.clone()),
            );
            payload.insert(
                "source_type".to_string(),
                Value::String(args.source_type.clone()),
            );
            payload.insert(
                "source_id".to_string(),
                Value::String(args.source_id.clone()),
            );
            insert_optional_string(&mut payload, "label", args.label.as_deref());
            insert_optional_string(&mut payload, "url", args.url.as_deref());
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(cli, "/api/cli/source-links", Value::Object(payload))?;
            render_assistant_value(
                cli.effective_output(),
                "source_links.add",
                "Source link",
                value,
            )
        }
        SourceLinkCommands::List(args) => {
            if args.limit == 0 {
                return Err(CliError::config("--limit must be at least 1."));
            }
            let mut query = Vec::new();
            push_query_opt(&mut query, "target_type", args.target_type.as_deref());
            push_query_opt(&mut query, "target_id", args.target_id.as_deref());
            if args.include_removed {
                push_query(&mut query, "include_removed", "true");
            }
            push_query(&mut query, "limit", args.limit.to_string());
            push_query(&mut query, "offset", args.offset.to_string());
            let value = get_assistant_json(cli, "/api/cli/source-links", &query)?;
            render_assistant_value(
                cli.effective_output(),
                "source_links.list",
                "Source links",
                value,
            )
        }
        SourceLinkCommands::Remove(args) => {
            let mut payload = Map::new();
            payload.insert("confirm".to_string(), json!(args.confirm));
            insert_agent_write_fields(&mut payload, &args.write)?;
            let value = post_assistant_json(
                cli,
                &format!(
                    "/api/cli/source-links/{}/remove",
                    encode_path_segment(&args.id)
                ),
                Value::Object(payload),
            )?;
            render_assistant_value(
                cli.effective_output(),
                "source_links.remove",
                "Source link",
                value,
            )
        }
    }
}

fn render_remote_status(
    cli: &Cli,
    event_type: &'static str,
    subject: &'static str,
    primary_path: &str,
    fallback_path: &str,
) -> Result<String, CliError> {
    let value = get_json_with_fallback(cli, primary_path, fallback_path)?;
    render_status_value(cli.effective_output(), event_type, subject, value)
}

fn get_json_with_fallback(
    cli: &Cli,
    primary_path: &str,
    fallback_path: &str,
) -> Result<Value, CliError> {
    let context = remote_context(cli, false)?;
    match get_json(&context, primary_path) {
        Ok(value) => Ok(value),
        Err(error) if matches!(error.status(), Some(404 | 405)) => {
            get_json(&context, fallback_path)
        }
        Err(error) => Err(error),
    }
}

fn remote_context(cli: &Cli, require_mesh_id: bool) -> Result<RemoteContext, CliError> {
    remote_context_with_timeout(cli, require_mesh_id, Duration::from_secs(30))
}

fn remote_context_with_timeout(
    cli: &Cli,
    require_mesh_id: bool,
    timeout: Duration,
) -> Result<RemoteContext, CliError> {
    let config_path = expand_tilde(&cli.config);
    let config = ConfigFile::load(&config_path)?;
    let profile_name = profile_name(cli, &config);
    let profile = config.profiles.get(&profile_name);
    let token = resolve_token(cli, profile);
    let now = now_unix_seconds();

    let Some(access_token) = token
        .access_token
        .as_ref()
        .filter(|token| !token.is_empty())
    else {
        return Err(CliError::auth_required(
            "Not authenticated. Use `smesh auth login --access-token <token>` or set SMESH_TOKEN.",
        ));
    };
    if token.expires_at.is_some_and(|expiry| expiry <= now) {
        return Err(CliError::auth_required(
            "Access token is present but expired. Refresh it or pass --token.",
        ));
    }

    let mesh_id = resolve_mesh_id(cli, profile);
    if require_mesh_id && mesh_id.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::config(
            "Missing mesh context. Pass --mesh-id, set SMESH_MESH_ID, or store mesh_id in the selected profile.",
        ));
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| CliError::network(format!("Failed to build HTTP client: {error}")))?;

    Ok(RemoteContext {
        client,
        api_url: resolve_api_url(cli, profile),
        access_token: access_token.clone(),
        mesh_id,
    })
}

fn get_json(context: &RemoteContext, path: &str) -> Result<Value, CliError> {
    let url = format!("{}{}", context.api_url.trim_end_matches('/'), path);
    let request = context
        .authenticated(context.client.get(url))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, USER_AGENT);

    send_json_request(request)
}

fn post_json_with_fallback(
    context: &RemoteContext,
    primary_path: &str,
    fallback_path: &str,
    payload: Value,
) -> Result<Value, CliError> {
    match post_json(context, primary_path, &payload) {
        Ok(value) => Ok(value),
        Err(error) if matches!(error.status(), Some(404 | 405)) => {
            post_json(context, fallback_path, &payload)
        }
        Err(error) => Err(error),
    }
}

fn post_json(context: &RemoteContext, path: &str, payload: &Value) -> Result<Value, CliError> {
    let url = format!("{}{}", context.api_url.trim_end_matches('/'), path);
    let request = context
        .authenticated(context.client.post(url))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(payload);

    send_json_request(request)
}

fn post_cli_json_with_fallback(
    cli: &Cli,
    primary_path: &str,
    payload: &Value,
    fallback_path: &str,
    timeout: Duration,
) -> Result<Value, CliError> {
    let context = remote_context_with_timeout(cli, true, timeout)?;
    match post_json(&context, primary_path, payload) {
        Ok(value) => Ok(value),
        Err(error) if matches!(error.status(), Some(404 | 405)) => {
            post_json(&context, fallback_path, payload)
        }
        Err(error) => Err(error),
    }
}

fn post_cli_json(
    cli: &Cli,
    path: &str,
    payload: &Value,
    timeout: Duration,
) -> Result<Value, CliError> {
    let context = remote_context_with_timeout(cli, true, timeout)?;
    post_json(&context, path, payload)
}

fn get_assistant_json(
    cli: &Cli,
    path: &str,
    query: &[(String, String)],
) -> Result<Value, CliError> {
    let context = remote_context_with_timeout(cli, true, Duration::from_secs(30))?;
    get_json(&context, &path_with_query(path, query))
}

fn post_assistant_json(cli: &Cli, path: &str, mut payload: Value) -> Result<Value, CliError> {
    let context = remote_context_with_timeout(cli, true, Duration::from_secs(30))?;
    insert_context_mesh_id(&mut payload, &context);
    post_json(&context, path, &payload)
}

fn patch_assistant_json(cli: &Cli, path: &str, mut payload: Value) -> Result<Value, CliError> {
    let context = remote_context_with_timeout(cli, true, Duration::from_secs(30))?;
    insert_context_mesh_id(&mut payload, &context);
    patch_json(&context, path, &payload)
}

fn put_assistant_json(cli: &Cli, path: &str, mut payload: Value) -> Result<Value, CliError> {
    let context = remote_context_with_timeout(cli, true, Duration::from_secs(30))?;
    insert_context_mesh_id(&mut payload, &context);
    put_json(&context, path, &payload)
}

fn patch_json(context: &RemoteContext, path: &str, payload: &Value) -> Result<Value, CliError> {
    let url = format!("{}{}", context.api_url.trim_end_matches('/'), path);
    let request = context
        .authenticated(context.client.patch(url))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(payload);

    send_json_request(request)
}

fn put_json(context: &RemoteContext, path: &str, payload: &Value) -> Result<Value, CliError> {
    let url = format!("{}{}", context.api_url.trim_end_matches('/'), path);
    let request = context
        .authenticated(context.client.put(url))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(payload);

    send_json_request(request)
}

fn post_multipart_with_fallback(
    context: &RemoteContext,
    primary_path: &str,
    fallback_path: &str,
    upload: MultipartUpload<'_>,
) -> Result<Value, CliError> {
    match post_multipart(context, primary_path, upload) {
        Ok(value) => Ok(value),
        Err(error) if matches!(error.status(), Some(404 | 405)) => {
            post_multipart(context, fallback_path, upload)
        }
        Err(error) => Err(error),
    }
}

fn post_multipart(
    context: &RemoteContext,
    path: &str,
    upload: MultipartUpload<'_>,
) -> Result<Value, CliError> {
    let boundary = multipart_boundary();
    let body = build_multipart_body(&boundary, upload)?;
    let url = format!("{}{}", context.api_url.trim_end_matches('/'), path);
    let request = context
        .authenticated(context.client.post(url))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body);

    send_json_request(request)
}

fn send_json_request(request: reqwest::blocking::RequestBuilder) -> Result<Value, CliError> {
    let response = request
        .send()
        .map_err(|error| CliError::network(format!("HTTP request failed: {error}")))?;
    let status = response.status();
    let status_code = status.as_u16();
    let text = response
        .text()
        .map_err(|error| CliError::network(format!("Failed to read HTTP response: {error}")))?;

    let parsed = serde_json::from_str::<Value>(&text).ok();
    if status.is_success() {
        return parsed.ok_or_else(|| {
            CliError::http(
                status_code,
                "Backend returned a non-JSON status response.",
                None,
            )
        });
    }

    let message = parsed
        .as_ref()
        .and_then(http_error_message)
        .unwrap_or_else(|| format!("HTTP {status_code}"));
    Err(CliError::http(status_code, message, parsed))
}

#[derive(Debug)]
struct RemoteContext {
    client: reqwest::blocking::Client,
    api_url: String,
    access_token: String,
    mesh_id: Option<String>,
}

impl RemoteContext {
    fn authenticated(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        let request = request.bearer_auth(&self.access_token);
        if let Some(mesh_id) = self
            .mesh_id
            .as_deref()
            .filter(|mesh_id| !mesh_id.is_empty())
        {
            request.header("X-Mesh-Context", mesh_id)
        } else {
            request
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MultipartUpload<'a> {
    file_field: &'a str,
    filename: &'a str,
    content_type: &'a str,
    file_content: &'a [u8],
    fields: &'a [(String, String)],
}

fn http_error_message(value: &Value) -> Option<String> {
    for key in ["detail", "error", "message"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn render_status_value(
    mode: OutputMode,
    event_type: &'static str,
    subject: &'static str,
    value: Value,
) -> Result<String, CliError> {
    match mode {
        OutputMode::Human => Ok(render_status_human(subject, &value)),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&value)?)),
        OutputMode::Ndjson => {
            let mut event = match value {
                Value::Object(map) => map,
                other => {
                    let mut map = serde_json::Map::new();
                    map.insert("payload".to_string(), other);
                    map
                }
            };
            event.insert("type".to_string(), Value::String(event_type.to_string()));
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_status_human(subject: &str, value: &Value) -> String {
    let id_key = if subject == "Job" {
        "job_id"
    } else {
        "capture_id"
    };
    let id = value_str(value, id_key)
        .or_else(|| value_str(value, "job_id"))
        .or_else(|| value_str(value, "capture_id"))
        .unwrap_or("unknown");
    let state = value_str(value, "processing_state")
        .or_else(|| value_str(value, "status"))
        .unwrap_or("unknown");

    let mut lines = vec![format!("{subject} {id}: {state}")];
    if let Some(capture_id) = value_str(value, "capture_id") {
        if subject != "Capture" {
            lines.push(format!("Capture: {capture_id}"));
        }
    }
    if let Some(task_type) = value_str(value, "task_type") {
        let status = value_str(value, "status").unwrap_or("unknown");
        lines.push(format!("Task: {task_type} ({status})"));
    }

    let source_ids = value_string_array(value, "source_ids");
    if !source_ids.is_empty() {
        lines.push(format!("Sources: {}", source_ids.join(", ")));
    }
    let file_ids = value_string_array(value, "file_ids");
    if !file_ids.is_empty() {
        lines.push(format!("File ID(s): {}", file_ids.join(", ")));
    }
    let source_links = value_display_array(value, "source_links");
    if !source_links.is_empty() {
        lines.push(format!("Source links: {}", source_links.join(", ")));
    }
    let memory_ids = value_string_array(value, "memory_ids");
    if !memory_ids.is_empty() {
        lines.push(format!("Memories: {}", memory_ids.join(", ")));
    }

    if let Some(files) = value.get("files").and_then(Value::as_array) {
        if !files.is_empty() {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for file in files {
                let status = value_str(file, "status").unwrap_or("unknown");
                *counts.entry(status.to_string()).or_default() += 1;
            }
            let summary = counts
                .into_iter()
                .map(|(status, count)| format!("{status}: {count}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("Files: {} total ({summary})", files.len()));
        }
    }

    let errors = value_string_array(value, "errors");
    if !errors.is_empty() {
        lines.push(format!("Errors: {}", errors.join("; ")));
    } else if let Some(error) = value_str(value, "error") {
        lines.push(format!("Error: {error}"));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn render_capture_enqueue_value(
    mode: OutputMode,
    event_type: &'static str,
    value: Value,
) -> Result<String, CliError> {
    match mode {
        OutputMode::Human => Ok(render_capture_enqueue_human(&value)),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&value)?)),
        OutputMode::Ndjson => {
            let mut event = match value {
                Value::Object(map) => map,
                other => {
                    let mut map = Map::new();
                    map.insert("payload".to_string(), other);
                    map
                }
            };
            event.insert("type".to_string(), Value::String(event_type.to_string()));
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_retrieval_output<T: Serialize>(
    mode: OutputMode,
    event_type: &'static str,
    output: &T,
    human: String,
) -> Result<String, CliError> {
    match mode {
        OutputMode::Human => Ok(human),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(output)?)),
        OutputMode::Ndjson => {
            let mut event = match serde_json::to_value(output)? {
                Value::Object(map) => map,
                other => {
                    let mut map = Map::new();
                    map.insert("payload".to_string(), other);
                    map
                }
            };
            event.insert("type".to_string(), Value::String(event_type.to_string()));
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_capture_enqueue_human(value: &Value) -> String {
    let mut lines = vec!["Capture queued.".to_string()];

    if let Some(operation_id) = value_str(value, "operation_id") {
        lines.push(format!("Operation ID: {operation_id}"));
    }
    if let Some(status) = value_str(value, "status") {
        lines.push(format!("Status: {status}"));
    }
    if let Some(job_id) = value_str(value, "job_id").or_else(|| value_str(value, "task_id")) {
        lines.push(format!("Job ID: {job_id}"));
    }
    if let Some(capture_id) = value_str(value, "capture_id") {
        lines.push(format!("Capture ID: {capture_id}"));
    }

    let file_ids = value_string_array(value, "file_ids");
    if !file_ids.is_empty() {
        lines.push(format!("File ID(s): {}", file_ids.join(", ")));
    }

    let details = value.get("details").unwrap_or(&Value::Null);
    if let Some(filename) = value_str(details, "filename").or_else(|| value_str(value, "filename"))
    {
        let content_type =
            value_str(details, "content_type").or_else(|| value_str(value, "content_type"));
        let suffix = content_type
            .map(|content_type| format!(" ({content_type})"))
            .unwrap_or_default();
        lines.push(format!("File: {filename}{suffix}"));
    }

    let mut tag_values = value_string_array(value, "tags");
    tag_values.extend(value_string_array(details, "tags"));
    let tags = normalize_tags(&tag_values);
    if !tags.is_empty() {
        lines.push(format!("Tags: {}", tags.join(", ")));
    }

    let source_links = value_display_array(value, "source_links");
    if !source_links.is_empty() {
        lines.push(format!("Source links: {}", source_links.join(", ")));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn normalize_search_output(
    query: String,
    top_k: usize,
    filters: SearchOutputFilters,
    value: &Value,
) -> SearchOutput {
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let normalized_results = results
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let null = Value::Null;
            let data_ref = item.get("data").unwrap_or(&null);
            SearchResultOutput {
                rank: index + 1,
                id: first_scalar_string(item, &["id", "node_id", "source_id"])
                    .or_else(|| first_scalar_string(data_ref, &["id", "node_id", "source_id"])),
                node_type: first_scalar_string(item, &["node_type", "type"])
                    .or_else(|| first_scalar_string(data_ref, &["node_type", "type"]))
                    .unwrap_or_else(|| "Unknown".to_string()),
                title: first_scalar_string(item, &["title", "name"])
                    .or_else(|| first_scalar_string(data_ref, &["title", "name"])),
                snippet: first_scalar_string(item, &["snippet", "excerpt"])
                    .or_else(|| first_scalar_string(data_ref, &["snippet", "excerpt", "text"])),
                score: value_f64(item, "score"),
                distance: value_f64(item, "distance"),
                data: data_ref.clone(),
            }
        })
        .collect::<Vec<_>>();

    SearchOutput {
        schema_version: RETRIEVAL_SCHEMA_VERSION,
        query,
        top_k,
        filters,
        result_count: normalized_results.len(),
        results: normalized_results,
    }
}

fn normalize_ask_output(question: String, value: &Value) -> AskOutput {
    AskOutput {
        schema_version: RETRIEVAL_SCHEMA_VERSION,
        question,
        answer: first_scalar_string(value, &["answer", "text", "response"]).unwrap_or_default(),
        source_ids: string_array_from_value(value.get("source_ids")),
        sources: value
            .get("sources")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        report_ids: string_array_from_value(value.get("report_ids")),
        suggested_title: first_scalar_string(value, &["suggested_title"]),
    }
}

fn normalize_topic_query_output(
    topics: Vec<String>,
    match_mode: String,
    limit: usize,
    offset: usize,
    sort: String,
    window: TopicWindowOutput,
    exclude_node_types: Vec<String>,
    value: &Value,
) -> TopicQueryOutput {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let normalized_items = items
        .iter()
        .map(|item| TopicItemOutput {
            id: first_scalar_string(item, &["id"]).unwrap_or_default(),
            node_type: first_scalar_string(item, &["node_type"])
                .unwrap_or_else(|| "Unknown".to_string()),
            title: first_scalar_string(item, &["title"]),
            snippet: first_scalar_string(item, &["snippet"]).unwrap_or_default(),
            updated_at: first_scalar_string(item, &["updatedAt", "updated_at"]),
            created_at: first_scalar_string(item, &["createdAt", "created_at"]),
            matched_topics: string_array_from_value(item.get("matched_topics")),
        })
        .collect::<Vec<_>>();
    let paging_value = value.get("paging").unwrap_or(&Value::Null);
    let paging = TopicPagingOutput {
        limit: value_usize(paging_value, "limit").unwrap_or(limit),
        offset: value_usize(paging_value, "offset").unwrap_or(offset),
        total: value_usize(paging_value, "total").unwrap_or(normalized_items.len()),
        has_more: paging_value
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };

    TopicQueryOutput {
        schema_version: RETRIEVAL_SCHEMA_VERSION,
        topics,
        match_mode,
        limit,
        offset,
        sort,
        window,
        exclude_node_types,
        items: normalized_items,
        paging,
    }
}

fn normalize_topic_activity_output(
    topics: Vec<String>,
    match_mode: String,
    window: TopicWindowOutput,
    exclude_node_types: Vec<String>,
    value: &Value,
) -> TopicActivityOutput {
    let rows = value
        .get("by_node_type")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let by_node_type = rows
        .iter()
        .map(|row| TopicActivityRowOutput {
            node_type: first_scalar_string(row, &["node_type"])
                .unwrap_or_else(|| "Unknown".to_string()),
            count: value_usize(row, "count").unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    TopicActivityOutput {
        schema_version: RETRIEVAL_SCHEMA_VERSION,
        topics,
        match_mode,
        window,
        exclude_node_types,
        total: value_usize(value, "total").unwrap_or_default(),
        by_node_type,
    }
}

fn render_search_human(output: &SearchOutput) -> String {
    let mut lines = vec![format!(
        "{} result(s) for \"{}\"",
        output.result_count, output.query
    )];
    if !output.filters.labels.is_empty() {
        lines.push(format!("Filters: {}", output.filters.labels.join(", ")));
    }
    for item in output.results.iter().take(10) {
        let title = item
            .title
            .as_deref()
            .or(item.id.as_deref())
            .unwrap_or("(untitled)");
        lines.push(format!("{}. [{}] {title}", item.rank, item.node_type));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn render_ask_human(output: &AskOutput) -> String {
    let mut lines = if output.answer.trim().is_empty() {
        vec!["No answer returned.".to_string()]
    } else {
        vec![output.answer.trim().to_string()]
    };
    if !output.source_ids.is_empty() {
        lines.push(format!("Sources: {}", output.source_ids.join(", ")));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn render_topics_query_human(output: &TopicQueryOutput) -> String {
    let mut lines = vec![format!("Topic query: {} match(es)", output.paging.total)];
    for (index, item) in output.items.iter().take(10).enumerate() {
        let title = item.title.as_deref().unwrap_or(&item.id);
        let topics = if item.matched_topics.is_empty() {
            String::new()
        } else {
            format!(" (topics: {})", item.matched_topics.join(", "))
        };
        lines.push(format!(
            "{}. [{}] {title}{topics}",
            index + 1,
            item.node_type
        ));
    }
    lines.push(format!(
        "Paging: offset={} limit={} has_more={}",
        output.paging.offset, output.paging.limit, output.paging.has_more
    ));
    lines.push(String::new());
    lines.join("\n")
}

fn render_topics_activity_human(output: &TopicActivityOutput) -> String {
    let mut lines = vec![format!("Topic activity total: {}", output.total)];
    for row in &output.by_node_type {
        lines.push(format!("- {}: {}", row.node_type, row.count));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn render_assistant_value(
    mode: OutputMode,
    event_type: &'static str,
    subject: &'static str,
    value: Value,
) -> Result<String, CliError> {
    match mode {
        OutputMode::Human => Ok(render_assistant_human(subject, &value)),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&value)?)),
        OutputMode::Ndjson => {
            let mut event = match value {
                Value::Object(map) => map,
                other => {
                    let mut map = Map::new();
                    map.insert("payload".to_string(), other);
                    map
                }
            };
            event.insert("type".to_string(), Value::String(event_type.to_string()));
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_assistant_human(subject: &str, value: &Value) -> String {
    let value = value.get("data").unwrap_or(value);
    let mut lines = vec![format!("{subject} result ready.")];
    for key in [
        "project",
        "assertion",
        "task",
        "reminder",
        "person",
        "org",
        "preference",
        "brief",
        "event",
        "source_link",
    ] {
        if let Some(item) = value.get(key).and_then(Value::as_object) {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                lines.push(format!("ID: {id}"));
            }
            if let Some(title) = item
                .get("title")
                .or_else(|| item.get("display_name"))
                .or_else(|| item.get("key"))
                .and_then(Value::as_str)
            {
                lines.push(format!("Name: {title}"));
            }
            if let Some(status) = item
                .get("status")
                .or_else(|| item.get("state"))
                .and_then(Value::as_str)
            {
                lines.push(format!("Status: {status}"));
            }
            break;
        }
    }
    for key in [
        "projects",
        "assertions",
        "tasks",
        "reminders",
        "people",
        "orgs",
        "preferences",
        "events",
        "source_links",
        "open_loops",
    ] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            lines.push(format!("{}: {}", title_case_ascii(key), items.len()));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

fn insert_optional_string(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = clean_optional(value) {
        map.insert(key.to_string(), Value::String(value));
    }
}

fn insert_string_array(map: &mut Map<String, Value>, key: &str, values: Vec<String>) {
    if !values.is_empty() {
        map.insert(
            key.to_string(),
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }
}

fn insert_agent_write_fields(
    map: &mut Map<String, Value>,
    write: &AgentWriteArgs,
) -> Result<(), CliError> {
    if let Some(actor) = clean_optional(write.actor.as_deref()) {
        map.insert("actor".to_string(), parse_actor_arg(&actor)?);
    }
    insert_optional_string(map, "idempotency_key", write.idempotency_key.as_deref());
    insert_optional_string(
        map,
        "preference_snapshot_id",
        write.preference_snapshot_id.as_deref(),
    );
    Ok(())
}

fn parse_actor_arg(actor: &str) -> Result<Value, CliError> {
    let Some((actor_type, actor_id)) = actor.split_once(':') else {
        return Err(CliError::config(
            "--actor must use TYPE:ID, for example --actor agent:pixel.",
        ));
    };
    let actor_type = actor_type.trim();
    let actor_id = actor_id.trim();
    if actor_type.is_empty() || actor_id.is_empty() {
        return Err(CliError::config(
            "--actor must include a non-empty type and id.",
        ));
    }
    Ok(json!({"type": actor_type, "id": actor_id}))
}

fn parse_target_arg(target: &str) -> Result<Value, CliError> {
    let Some((target_type, target_id)) = target.split_once(':') else {
        return Err(CliError::config(
            "--target must use TYPE:ID, for example --target project:scientia_dev.",
        ));
    };
    let target_type = target_type.trim();
    let target_id = target_id.trim();
    if target_type.is_empty() || target_id.is_empty() {
        return Err(CliError::config(
            "--target must include a non-empty type and id.",
        ));
    }
    Ok(json!({ "type": target_type, "id": target_id }))
}

fn parse_preference_value(args: &PreferenceSetArgs) -> Result<Value, CliError> {
    if let Some(raw) = clean_optional(args.value_json.as_deref()) {
        return serde_json::from_str(&raw)
            .map_err(|error| CliError::config(format!("Invalid --value-json payload: {error}")));
    }
    if let Some(raw) = clean_optional(args.value.as_deref()) {
        return Ok(Value::String(raw));
    }
    Err(CliError::config(
        "Specify either --value or --value-json for preferences set.",
    ))
}

fn push_query(query: &mut Vec<(String, String)>, key: &str, value: impl Into<String>) {
    query.push((key.to_string(), value.into()));
}

fn push_query_opt(query: &mut Vec<(String, String)>, key: &str, value: Option<&str>) {
    if let Some(value) = clean_optional(value) {
        push_query(query, key, value);
    }
}

fn push_query_values(query: &mut Vec<(String, String)>, key: &str, values: Vec<String>) {
    if !values.is_empty() {
        push_query(query, key, values.join(","));
    }
}

fn path_with_query(path: &str, query: &[(String, String)]) -> String {
    if query.is_empty() {
        return path.to_string();
    }
    let encoded = query
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{path}?{encoded}")
}

fn insert_context_mesh_id(payload: &mut Value, context: &RemoteContext) {
    let Some(mesh_id) = context
        .mesh_id
        .as_deref()
        .filter(|mesh_id| !mesh_id.is_empty())
    else {
        return;
    };
    if let Value::Object(map) = payload {
        map.entry("mesh_id".to_string())
            .or_insert_with(|| Value::String(mesh_id.to_string()));
    }
}

fn parse_scope_arg(scope: &str) -> Value {
    let scope = scope.trim();
    if let Some((scope_type, id)) = scope.split_once(':') {
        json!({"type": scope_type, "id": id})
    } else {
        json!({"type": scope})
    }
}

fn joined_words(words: &[String]) -> String {
    words.join(" ").trim().to_string()
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn collect_repeated_values(values: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for raw in values {
        for item in raw.split(',') {
            let item = item.trim();
            if !item.is_empty() && !result.iter().any(|seen| seen == item) {
                result.push(item.to_string());
            }
        }
    }
    result
}

fn topic_window(args: &TopicCommonArgs) -> TopicWindowOutput {
    TopicWindowOutput {
        since: clean_optional(args.since.as_deref()),
        until: clean_optional(args.until.as_deref()),
    }
}

fn topic_window_payload(window: &TopicWindowOutput) -> Option<Value> {
    let mut payload = Map::new();
    if let Some(since) = &window.since {
        payload.insert("since".to_string(), Value::String(since.clone()));
    }
    if let Some(until) = &window.until {
        payload.insert("until".to_string(), Value::String(until.clone()));
    }
    if payload.is_empty() {
        None
    } else {
        Some(Value::Object(payload))
    }
}

fn first_scalar_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| value.get(*key))
        .filter_map(scalar_string)
        .find(|value| !value.is_empty())
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn string_array_from_value(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(scalar_string)
                .filter(|text| !text.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn value_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn value_usize(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

#[derive(Clone, Copy, Debug)]
struct CaptureResponseContext<'a> {
    capture_type: &'a str,
    mesh_id: &'a str,
    filename: Option<&'a str>,
    content_type: Option<&'a str>,
    tags: &'a [String],
}

fn normalize_capture_response(value: Value, context: CaptureResponseContext<'_>) -> Value {
    let Value::Object(mut map) = value else {
        return value;
    };

    if !map.contains_key("job_id") {
        if let Some(task_id) = map.get("task_id").cloned() {
            map.insert("job_id".to_string(), task_id);
        }
    }
    if !map.contains_key("operation_id") {
        let operation_id = first_string_in_map(
            &map,
            &["operation_id", "job_id", "task_id", "capture_id", "file_id"],
        )
        .unwrap_or_else(|| local_operation_id(&format!("capture.{}", context.capture_type)));
        map.insert("operation_id".to_string(), Value::String(operation_id));
    }
    map.entry("status".to_string())
        .or_insert_with(|| Value::String("queued".to_string()));
    map.entry("message".to_string()).or_insert_with(|| {
        Value::String(format!(
            "{} capture queued.",
            title_case_ascii(context.capture_type)
        ))
    });

    let mut details = map
        .remove("details")
        .and_then(|details| match details {
            Value::Object(details) => Some(details),
            _ => None,
        })
        .unwrap_or_default();

    details
        .entry("capture_type".to_string())
        .or_insert_with(|| Value::String(context.capture_type.to_string()));
    details
        .entry("mesh_id".to_string())
        .or_insert_with(|| Value::String(context.mesh_id.to_string()));
    if let Some(filename) = context.filename {
        details
            .entry("filename".to_string())
            .or_insert_with(|| Value::String(filename.to_string()));
    }
    if let Some(content_type) = context.content_type {
        details
            .entry("content_type".to_string())
            .or_insert_with(|| Value::String(content_type.to_string()));
    }
    if !context.tags.is_empty() {
        let tags = Value::Array(context.tags.iter().cloned().map(Value::String).collect());
        details
            .entry("tags".to_string())
            .or_insert_with(|| tags.clone());
        map.entry("tags".to_string()).or_insert(tags);
    }

    let file_id = map
        .get("file_id")
        .and_then(Value::as_str)
        .or_else(|| details.get("file_id").and_then(Value::as_str))
        .or_else(|| map.get("source_file_id").and_then(Value::as_str))
        .filter(|file_id| !file_id.is_empty())
        .map(ToOwned::to_owned);
    if !map.contains_key("file_id") {
        if let Some(file_id) = file_id.as_ref() {
            map.insert("file_id".to_string(), Value::String(file_id.clone()));
        }
    }
    let file_ids = map
        .get("file_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| !item.is_empty())
                .map(|item| Value::String(item.to_string()))
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .or_else(|| file_id.map(|file_id| vec![Value::String(file_id)]))
        .unwrap_or_default();
    map.insert("file_ids".to_string(), Value::Array(file_ids));

    let source_links = map
        .get("source_links")
        .cloned()
        .filter(Value::is_array)
        .or_else(|| {
            details
                .get("source_links")
                .or_else(|| details.get("sources"))
                .cloned()
                .filter(Value::is_array)
        })
        .unwrap_or_else(|| Value::Array(Vec::new()));
    map.insert("source_links".to_string(), source_links);

    let mut links = map
        .remove("links")
        .and_then(|links| match links {
            Value::Object(links) => Some(links),
            _ => None,
        })
        .unwrap_or_default();
    if let Some(capture_id) = map.get("capture_id").and_then(Value::as_str) {
        links
            .entry("capture".to_string())
            .or_insert_with(|| Value::String(format!("/v1/captures/{capture_id}")));
    }
    if let Some(job_id) = map.get("job_id").and_then(Value::as_str) {
        links
            .entry("job".to_string())
            .or_insert_with(|| Value::String(format!("/v1/jobs/{job_id}")));
    }
    if !links.is_empty() {
        map.insert("links".to_string(), Value::Object(links));
    }

    map.insert("details".to_string(), Value::Object(details));
    Value::Object(map)
}

fn first_string_in_map(map: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| map.get(*key))
        .filter_map(Value::as_str)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn value_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn value_display_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) if !text.is_empty() => Some(text.to_string()),
                    Value::Object(map) => map
                        .get("url")
                        .or_else(|| map.get("href"))
                        .or_else(|| map.get("id"))
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .map(ToOwned::to_owned),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn encode_path_segment(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn normalize_tags(values: &[String]) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = BTreeMap::<String, ()>::new();
    for raw in values {
        for part in raw.split(',') {
            let tag = part.trim().trim_start_matches('#').trim();
            if tag.is_empty() {
                continue;
            }
            let key = tag.to_lowercase();
            if seen.contains_key(&key) {
                continue;
            }
            seen.insert(key, ());
            tags.push(tag.to_string());
        }
    }
    tags
}

fn non_empty_string(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn detect_mime_type(path: &Path, override_type: Option<&str>, content: &[u8]) -> String {
    if let Some(explicit) = non_empty_string(override_type) {
        return explicit.to_string();
    }

    let detected = detect_mime_from_bytes(content);
    if !matches!(detected, "text/plain" | "application/octet-stream") {
        return detected.to_string();
    }

    if let Some(mime) = path.extension().and_then(|extension| {
        extension
            .to_str()
            .and_then(|extension| extension_mime(extension))
    }) {
        return mime.to_string();
    }

    detected.to_string()
}

fn extension_mime(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "avif" => Some("image/avif"),
        "csv" => Some("text/csv"),
        "gif" => Some("image/gif"),
        "htm" | "html" => Some("text/html"),
        "jpeg" | "jpg" => Some("image/jpeg"),
        "json" => Some("application/json"),
        "md" | "markdown" => Some("text/markdown"),
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "svg" => Some("image/svg+xml"),
        "txt" | "text" => Some("text/plain"),
        "webp" => Some("image/webp"),
        "zip" => Some("application/zip"),
        _ => None,
    }
}

fn detect_mime_from_bytes(content: &[u8]) -> &'static str {
    if content.starts_with(b"\xFF\xD8\xFF") {
        return "image/jpeg";
    }
    if content.starts_with(b"\x89PNG\r\n\x1A\n") {
        return "image/png";
    }
    if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if content.len() >= 12 && &content[..4] == b"RIFF" && &content[8..12] == b"WEBP" {
        return "image/webp";
    }
    if content.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    if content.starts_with(b"PK\x03\x04") {
        return "application/zip";
    }

    let stripped = trim_ascii_start(content);
    let svg_probe = stripped.get(..stripped.len().min(128)).unwrap_or(stripped);
    if svg_probe.starts_with(b"<svg") || svg_probe.windows(4).any(|window| window == b"<svg") {
        return "image/svg+xml";
    }
    if stripped.starts_with(b"{") || stripped.starts_with(b"[") {
        return "application/json";
    }
    if std::str::from_utf8(content).is_ok() {
        return "text/plain";
    }

    "application/octet-stream"
}

fn trim_ascii_start(value: &[u8]) -> &[u8] {
    let index = value
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(value.len());
    &value[index..]
}

fn build_multipart_body(boundary: &str, upload: MultipartUpload<'_>) -> Result<Vec<u8>, CliError> {
    let mut body = Vec::new();
    for (name, value) in upload.fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                multipart_quote(name)
            )
            .as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"; filename*=UTF-8''{}\r\n",
            multipart_quote(upload.file_field),
            multipart_quote(upload.filename),
            percent_encode(upload.filename)
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", upload.content_type).as_bytes());
    body.extend_from_slice(upload.file_content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok(body)
}

fn multipart_boundary() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("----smesh{}{}", std::process::id(), nanos)
}

fn multipart_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn title_case_ascii(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_ascii_uppercase().to_string() + chars.as_str()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthStatusOutput {
    pub authenticated: bool,
    pub access_token_present: bool,
    pub access_token_expired: bool,
    pub audience: Option<String>,
    pub domain: Option<String>,
    pub api_url: String,
    pub mesh_id: Option<String>,
    pub sub: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthLoginOutput {
    pub operation_id: String,
    pub status: &'static str,
    pub profile: String,
    pub api_url: String,
    pub mesh_id: Option<String>,
    pub access_token_present: bool,
    pub refresh_token_present: bool,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthLogoutOutput {
    pub operation_id: String,
    pub status: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AuthStatusEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    #[serde(flatten)]
    status: AuthStatusOutput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AuthLoginEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    #[serde(flatten)]
    login: AuthLoginOutput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AuthLogoutEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    #[serde(flatten)]
    logout: AuthLogoutOutput,
}

fn render_auth_command(cli: &Cli, command: &AuthCommands) -> Result<String, CliError> {
    match command {
        AuthCommands::Status => render_auth_status(cli),
        AuthCommands::Login(args) => render_auth_login(cli, args),
        AuthCommands::Logout => render_auth_logout(cli),
    }
}

fn render_auth_status(cli: &Cli) -> Result<String, CliError> {
    let config_path = expand_tilde(&cli.config);
    let config = ConfigFile::load(&config_path)?;
    let profile_name = profile_name(cli, &config);
    let profile = config.profiles.get(&profile_name);
    let status = auth_status(cli, profile);

    match cli.effective_output() {
        OutputMode::Human => Ok(render_auth_status_human(&status)),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&status)?)),
        OutputMode::Ndjson => {
            let event = AuthStatusEvent {
                event_type: "auth.status",
                status,
            };
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_auth_login(cli: &Cli, args: &AuthLoginArgs) -> Result<String, CliError> {
    let Some(access_token) = args.access_token.clone() else {
        return Err(CliError::unsupported(
            "Device flow is not supported for this client. Browser login is not yet wired in the Rust CLI. Use `smesh auth login --access-token <token>` or set SMESH_TOKEN for non-interactive commands.",
        ));
    };

    let config_path = expand_tilde(&cli.config);
    let mut config = ConfigFile::load(&config_path)?;
    let profile_name = profile_name(cli, &config);
    config.active_profile = profile_name.clone();

    let profile = config.profiles.entry(profile_name.clone()).or_default();
    let api_url = resolve_api_url(cli, Some(profile));
    let mesh_id = resolve_mesh_id(cli, Some(profile));
    let domain = args
        .auth_domain
        .clone()
        .or_else(|| {
            env_first(&[
                "SMESH_AUTH0_DOMAIN",
                "AUTH0_DOMAIN",
                "NEXT_PUBLIC_AUTH0_DOMAIN",
            ])
        })
        .or_else(|| {
            profile
                .auth_settings
                .as_ref()
                .and_then(|settings| settings.domain.clone())
        })
        .unwrap_or_else(|| DEFAULT_AUTH_DOMAIN.to_string());
    let audience = args
        .auth_audience
        .clone()
        .or_else(|| {
            env_first(&[
                "SMESH_AUTH0_AUDIENCE",
                "AUTH0_AUDIENCE",
                "NEXT_PUBLIC_AUTH0_AUDIENCE",
            ])
        })
        .or_else(|| {
            profile
                .auth_settings
                .as_ref()
                .and_then(|settings| settings.audience.clone())
        })
        .unwrap_or_else(|| DEFAULT_AUTH_AUDIENCE.to_string());
    let client_id = args
        .auth_client_id
        .clone()
        .or_else(|| {
            env_first(&[
                "SMESH_AUTH0_CLIENT_ID",
                "AUTH0_CLI_CLIENT_ID",
                "AUTH0_MCP_CLIENT_ID",
                "NEXT_PUBLIC_AUTH0_CLIENT_ID",
                "AUTH0_CLIENT_ID",
            ])
        })
        .or_else(|| {
            profile
                .auth_settings
                .as_ref()
                .and_then(|settings| settings.client_id.clone())
        })
        .unwrap_or_else(|| DEFAULT_AUTH_CLIENT_ID.to_string());

    profile.api_url = Some(api_url.clone());
    profile.mesh_id = mesh_id.clone();
    profile.auth = Some(AuthConfig {
        access_token: Some(access_token),
        refresh_token: args.refresh_token.clone(),
        expires_at: args.expires_at,
        token_type: Some("Bearer".to_string()),
        extra: BTreeMap::new(),
    });
    profile.auth_settings = Some(AuthSettings {
        domain: Some(domain),
        client_id: Some(client_id),
        audience: Some(audience),
        extra: BTreeMap::new(),
    });
    config.write(&config_path)?;

    let output = AuthLoginOutput {
        operation_id: local_operation_id("auth.login"),
        status: "logged_in",
        profile: profile_name,
        api_url,
        mesh_id,
        access_token_present: true,
        refresh_token_present: args.refresh_token.is_some(),
        expires_at: args.expires_at,
    };

    match cli.effective_output() {
        OutputMode::Human => Ok(format!(
            "Logged in profile `{}` with a stored access token.\nOperation ID: {}\n",
            output.profile, output.operation_id
        )),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&output)?)),
        OutputMode::Ndjson => {
            let event = AuthLoginEvent {
                event_type: "auth.login",
                login: output,
            };
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_auth_logout(cli: &Cli) -> Result<String, CliError> {
    let config_path = expand_tilde(&cli.config);
    let mut config = ConfigFile::load(&config_path)?;
    let profile_name = profile_name(cli, &config);
    let should_write = if let Some(profile) = config.profiles.get_mut(&profile_name) {
        let had_auth = profile.auth.take().is_some();
        if let Some(settings) = profile.auth_settings.as_mut() {
            settings.extra.remove("access_token");
        }
        had_auth
    } else {
        false
    };

    if should_write {
        config.write(&config_path)?;
    }

    let output = AuthLogoutOutput {
        operation_id: local_operation_id("auth.logout"),
        status: "logged_out",
    };

    match cli.effective_output() {
        OutputMode::Human => Ok(format!(
            "Logged out.\nOperation ID: {}\n",
            output.operation_id
        )),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&output)?)),
        OutputMode::Ndjson => {
            let event = AuthLogoutEvent {
                event_type: "auth.logout",
                logout: output,
            };
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn render_auth_status_human(status: &AuthStatusOutput) -> String {
    if status.authenticated {
        format!(
            "Authenticated for {}{}\n",
            status.api_url,
            status
                .mesh_id
                .as_ref()
                .map(|mesh_id| format!(" with mesh {mesh_id}"))
                .unwrap_or_default()
        )
    } else if status.access_token_present && status.access_token_expired {
        "Access token is present but expired.\n".to_string()
    } else {
        "Not authenticated. Use `smesh auth login --access-token <token>` or set SMESH_TOKEN.\n"
            .to_string()
    }
}

fn auth_status(cli: &Cli, profile: Option<&ProfileConfig>) -> AuthStatusOutput {
    let token = resolve_token(cli, profile);
    let claims = JwtClaims::default();
    let expires_at = token.expires_at;
    let now = now_unix_seconds();
    let access_token_expired = expires_at.is_some_and(|expiry| expiry <= now);
    let access_token_present = token.access_token.is_some();

    AuthStatusOutput {
        authenticated: access_token_present && !access_token_expired,
        access_token_present,
        access_token_expired,
        audience: resolve_auth_setting(
            profile,
            |settings| settings.audience.as_deref(),
            &[
                "SMESH_AUTH0_AUDIENCE",
                "AUTH0_AUDIENCE",
                "NEXT_PUBLIC_AUTH0_AUDIENCE",
            ],
            DEFAULT_AUTH_AUDIENCE,
        ),
        domain: resolve_auth_setting(
            profile,
            |settings| settings.domain.as_deref(),
            &[
                "SMESH_AUTH0_DOMAIN",
                "AUTH0_DOMAIN",
                "NEXT_PUBLIC_AUTH0_DOMAIN",
            ],
            DEFAULT_AUTH_DOMAIN,
        ),
        api_url: resolve_api_url(cli, profile),
        mesh_id: resolve_mesh_id(cli, profile),
        sub: claims.sub,
        email: claims.email,
        name: claims.name,
    }
}

fn resolve_token(cli: &Cli, profile: Option<&ProfileConfig>) -> ResolvedToken {
    if let Some(token) = cli.token.as_ref().filter(|token| !token.is_empty()) {
        return ResolvedToken {
            access_token: Some(token.clone()),
            expires_at: None,
        };
    }

    if let Some(token) = env_first(&["SMESH_API_KEY", "AUTH0_ACCESS_TOKEN"]) {
        return ResolvedToken {
            access_token: Some(token),
            expires_at: None,
        };
    }

    let auth = profile.and_then(|profile| profile.auth.as_ref());
    ResolvedToken {
        access_token: auth.and_then(|auth| auth.access_token.clone()),
        expires_at: auth.and_then(|auth| auth.expires_at),
    }
}

fn resolve_api_url(cli: &Cli, profile: Option<&ProfileConfig>) -> String {
    if cli.api_url != DEFAULT_API_URL {
        return cli.api_url.clone();
    }

    profile
        .and_then(|profile| profile.api_url.clone())
        .unwrap_or_else(|| cli.api_url.clone())
}

fn resolve_mesh_id(cli: &Cli, profile: Option<&ProfileConfig>) -> Option<String> {
    cli.mesh_id
        .clone()
        .or_else(|| profile.and_then(|profile| profile.mesh_id.clone()))
}

fn resolve_auth_setting(
    profile: Option<&ProfileConfig>,
    profile_value: impl for<'a> Fn(&'a AuthSettings) -> Option<&'a str>,
    env_names: &[&str],
    default: &str,
) -> Option<String> {
    env_first(env_names)
        .or_else(|| {
            profile
                .and_then(|profile| profile.auth_settings.as_ref())
                .and_then(profile_value)
                .map(ToOwned::to_owned)
        })
        .or_else(|| Some(default.to_string()))
}

fn profile_name(cli: &Cli, config: &ConfigFile) -> String {
    if cli.profile == "default" && !config.active_profile.is_empty() {
        config.active_profile.clone()
    } else {
        cli.profile.clone()
    }
}

fn env_first(names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
}

fn expand_tilde(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    } else if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }

    path.to_path_buf()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn local_operation_id(kind: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!(
        "{}-{}-{nanos}",
        kind.replace('.', "-").replace('_', "-"),
        std::process::id()
    )
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ResolvedToken {
    access_token: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct JwtClaims {
    sub: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default = "default_config_version")]
    version: u8,
    #[serde(default = "default_profile_name")]
    active_profile: String,
    #[serde(default)]
    profiles: BTreeMap<String, ProfileConfig>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl ConfigFile {
    fn load(path: &Path) -> Result<Self, CliError> {
        match fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
                CliError::config(format!(
                    "Invalid ScientiaMesh config at {}: {error}",
                    path.display()
                ))
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(CliError::config(format!(
                "Failed to read ScientiaMesh config at {}: {error}",
                path.display()
            ))),
        }
    }

    fn write(&self, path: &Path) -> Result<(), CliError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                CliError::config(format!(
                    "Failed to create ScientiaMesh config directory {}: {error}",
                    parent.display()
                ))
            })?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        write_private_file(path, contents.as_bytes()).map_err(|error| {
            CliError::config(format!(
                "Failed to write ScientiaMesh config at {}: {error}",
                path.display()
            ))
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct ProfileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<AuthConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_settings: Option<AuthSettings>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct AuthConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_type: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct AuthSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audience: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn default_config_version() -> u8 {
    1
}

fn default_profile_name() -> String {
    "default".to_string()
}

#[cfg(unix)]
fn write_private_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    fs::write(path, contents)
}
