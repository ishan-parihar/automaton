use serde::{Deserialize, Serialize};

use crate::flow::FlowStep;
use crate::module::{ModuleId, RetryConfig};

/// A node in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub properties: serde_json::Map<String, serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// An edge in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub properties: serde_json::Map<String, serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Node types in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Module,
    Workflow,
    Trigger,
    Resource,
    SecretRef,
    Capability,
    Artifact,
    Run,
    Observation,
    Constraint,
    AlternativePath,
    Input,
}

/// Edge types in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    DependsOn,
    Calls,
    Emits,
    Consumes,
    Triggers,
    UsesResource,
    BlockedBy,
    AlternativeTo,
    Upgrades,
    DerivedFrom,
}

/// A module node in a materialized run graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleNode {
    pub id: String,
    pub module_id: ModuleId,
    pub input: serde_json::Value,
    pub retry: Option<RetryConfig>,
    pub timeout_ms: u64,
    pub depends_on: Vec<String>,
    pub parallel_group: Option<String>,
    pub condition: Option<String>,
    pub error_handler: Option<Box<ModuleNode>>,
}

/// A materialized run graph for a flow execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunGraph {
    pub id: String,
    pub workflow_name: String,
    pub modules: Vec<ModuleNode>,
    pub steps: Vec<FlowStep>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
