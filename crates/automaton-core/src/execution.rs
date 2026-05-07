use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Unique execution identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecutionId(pub String);

impl ExecutionId {
    pub fn new() -> Self {
        ExecutionId(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for ExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ExecutionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a single step during execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Skipped(String),
    TimedOut,
}

/// Telemetry for a single step execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTelemetry {
    pub step_id: String,
    pub step_kind: String,
    pub status: StepStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub retry_attempt: u32,
    pub output: Option<Value>,
    pub error: Option<String>,
}

/// Full execution record for a flow or DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowExecution {
    pub execution_id: ExecutionId,
    pub flow_path: Option<String>,
    pub dag_label: Option<String>,
    pub steps: Vec<StepTelemetry>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: StepStatus,
    pub total_duration_ms: Option<u64>,
}

/// Types of events that can trigger webhooks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WebhookEvent {
    FlowCompleted,
    FlowFailed,
    StepCompleted,
    StepFailed,
    DagCompleted,
    DagFailed,
    RunCompleted,
    BuildCompleted,
}

/// A registered webhook destination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRegistration {
    pub id: String,
    pub target_url: String,
    pub event: WebhookEvent,
    pub secret: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}
