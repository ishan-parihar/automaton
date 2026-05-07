use serde::{Deserialize, Serialize};

use crate::module::{ContentHash, RetryConfig};

/// A step in a flow DAG
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowStep {
    pub id: String,
    pub kind: FlowStepKind,
    pub script_path: Option<String>,
    pub input: serde_json::Value,
    pub retry: Option<RetryConfig>,
    pub timeout_ms: u64,
    pub depends_on: Vec<String>,
    pub sleep_after_ms: Option<u64>,
    pub stop_if: Option<String>,
    pub failure_step: Option<String>,
}

/// Flow step execution kind
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FlowStepKind {
    Script,
    Shell {
        command: String,
        shell: Option<String>,
    },
    BranchOne(Vec<Vec<FlowStep>>),
    BranchAll(Vec<Vec<FlowStep>>),
    ForLoop {
        iterator: String,
        steps: Vec<FlowStep>,
    },
    WhileLoop {
        condition: String,
        steps: Vec<FlowStep>,
        max_iterations: u32,
    },
    Sleep,
    FailureModule,
    /// Call another flow by path, merging its results
    CallFlow {
        flow_path: String,
        input: Option<serde_json::Value>,
    },
}

/// A complete flow definition — the composition of multiple steps
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowDefinition {
    /// Human-readable path, e.g. "social.daily_pipeline"
    pub path: String,
    pub version: String,
    pub summary: Option<String>,
    /// Ordered list of flow steps forming the DAG
    pub steps: Vec<FlowStep>,
    pub default_retry: Option<RetryConfig>,
    pub default_timeout_ms: u64,
    /// Step to run if a step fails
    pub on_failure: Option<String>,
    pub tags: Vec<String>,
}

impl Default for FlowDefinition {
    fn default() -> Self {
        Self {
            path: String::new(),
            version: "0.1.0".to_string(),
            summary: None,
            steps: vec![],
            default_retry: Some(RetryConfig::default()),
            default_timeout_ms: 30_000,
            on_failure: None,
            tags: vec![],
        }
    }
}

/// A stored flow with content hash
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Flow {
    pub definition: FlowDefinition,
    pub hash: ContentHash,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
