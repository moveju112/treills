use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const API_VERSION: &str = "1.0";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RunState {
    #[default] Unknown,
    Running,
    Recent,
    Waiting,
    Inactive,
    Success,
    Failed,
    Cancelled,
    Ended,
}
impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown", Self::Running => "running",
            Self::Recent => "recent", Self::Waiting => "waiting",
            Self::Inactive => "inactive", Self::Success => "success",
            Self::Failed => "failed", Self::Cancelled => "cancelled",
            Self::Ended => "ended",
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "running" => Self::Running, "recent" => Self::Recent,
            "waiting" => Self::Waiting, "inactive" => Self::Inactive,
            "success" => Self::Success, "failed" => Self::Failed,
            "cancelled" => Self::Cancelled, "ended" => Self::Ended,
            _ => Self::Unknown,
        }
    }
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Cancelled | Self::Ended)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Preview {
    pub workflow_id: String,
    pub name: String,
    pub state: RunState,
    pub node_count: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: String,
    pub active_count: i32,
    pub attention_count: i32,
    pub ended_count: i32,
    pub latest_at: Option<i64>,
    pub previews: Vec<Preview>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Workflow {
    pub id: String,
    pub project_id: String,
    pub source: String,
    pub session_id: String,
    pub turn_id: String,
    pub parent_turn_id: Option<String>,
    pub name: String,
    pub state: RunState,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub latest_at: Option<i64>,
    pub worktree: String,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub node_count: i32,
    pub incomplete: bool,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentNode {
    pub id: String,
    pub workflow_id: String,
    pub parent_id: Option<String>,
    pub child_session_id: Option<String>,
    pub role: String,
    pub model: Option<String>,
    pub state: RunState,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub latest_at: Option<i64>,
    pub last_tool: Option<String>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub evidence_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimelineEvent {
    pub id: String,
    pub workflow_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub node_id: String,
    pub at: Option<i64>,
    pub kind: String,
    pub detail: String,
    pub evidence_source: String,
    pub call_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IngestIssue {
    pub path: String,
    pub message: String,
    pub recorded_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Change {
    pub seq: i64,
    pub workflow_id: String,
    pub project_id: String,
    pub at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectsPage {
    pub api_version: String,
    pub items: Vec<Project>,
    pub next_offset: Option<i64>,
    pub last_change_id: i64,
    pub issues: Vec<IngestIssue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorkflowsPage {
    pub api_version: String,
    pub items: Vec<Workflow>,
    pub next_offset: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventsPage {
    pub api_version: String,
    pub items: Vec<TimelineEvent>,
    pub next_cursor: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorkflowDetail {
    pub api_version: String,
    pub workflow: Workflow,
    pub nodes: Vec<AgentNode>,
    pub edges: Vec<GraphEdge>,
    pub events: Vec<TimelineEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChangesPage {
    pub api_version: String,
    pub items: Vec<Change>,
    pub next_cursor: i64,
    pub has_more: bool,
}


#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionRecord {
    pub id: String,
    pub project_id: String,
    pub source: String,
    pub native_id: String,
    pub parent_turn_id: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub latest_at: Option<i64>,
    pub state: RunState,
    pub worktree: String,
    pub branch: Option<String>,
    pub incomplete: bool,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionsPage {
    pub api_version: String,
    pub items: Vec<SessionRecord>,
    pub next_offset: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Scope {
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
}

pub fn typescript() -> String {
    use ts_rs::TS;
    let pieces = [
        RunState::decl(), Preview::decl(), Project::decl(), Workflow::decl(),
        SessionRecord::decl(), SessionsPage::decl(), Scope::decl(),
        AgentNode::decl(), GraphEdge::decl(), TimelineEvent::decl(),
        IngestIssue::decl(), Change::decl(), ProjectsPage::decl(),
        WorkflowsPage::decl(), EventsPage::decl(), WorkflowDetail::decl(),
        ChangesPage::decl(),
    ];
    format!(
        "// Generated by 'cargo run -- typegen'. Do not edit.\n\
         // API version: {}\n{}\n",
        API_VERSION,
        pieces.into_iter().map(|s| format!("export {s}")).collect::<Vec<_>>().join("\n\n")
    )
}
