use serde::{Deserialize, Serialize};

/// A job queued for execution by a worker
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub id: i64,
    pub kind: JobKind,
    pub target_path: String,
    pub args: serde_json::Value,
    pub scheduled_for: chrono::DateTime<chrono::Utc>,
    pub priority: i32,
    pub running: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// What kind of job this is
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobKind {
    /// Run a single script module
    Script,
    /// Run a composed flow
    Flow,
    /// Execute a single step within a flow
    FlowStep,
    /// Identity — used for the identity worker
    Identity,
}

/// Result of a completed execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunResult {
    pub id: String,
    pub target_path: String,
    pub kind: JobKind,
    pub args: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub state: RunState,
    pub attempt: u32,
    pub duration_ms: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RunState {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}
