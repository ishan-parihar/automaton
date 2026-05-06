use serde::{Deserialize, Serialize};

use super::module::{ContentHash, RetryConfig};
use super::graph::FlowStep;

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
