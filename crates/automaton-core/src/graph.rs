use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::module::ModuleId;

/// Node types in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A compiled automation module
    Module,
    /// A composed workflow of multiple modules
    Workflow,
    /// A trigger (schedule, webhook, event)
    Trigger,
    /// An external resource binding
    Resource,
    /// A reference to a secret
    SecretRef,
    /// A declared capability
    Capability,
    /// An artifact produced by a run
    Artifact,
    /// A recorded run
    Run,
    /// An observation from a run (metrics, logs)
    Observation,
    /// A constraint on execution
    Constraint,
    /// An alternative execution path
    AlternativePath,
    /// An input parameter definition
    Input,
}

/// Edge types in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// A depends on B (A's execution requires B's output)
    DependsOn,
    /// A calls/invokes B
    Calls,
    /// A emits data to B
    Emits,
    /// A consumes data from B
    Consumes,
    /// A triggers B (event-driven)
    Triggers,
    /// A uses resource B
    UsesResource,
    /// A is blocked by B
    BlockedBy,
    /// B is an alternative to A
    AlternativeTo,
    /// B upgrades/replaces A
    Upgrades,
    /// B is derived from A
    DerivedFrom,
}

/// A node in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// An edge in the property graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub properties: HashMap<String, serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The persistent design graph — what exists in the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignGraph {
    pub name: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// A module node in a materialized run graph (for one execution)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleNode {
    pub id: String,
    pub module_id: ModuleId,
    pub input: serde_json::Value,
    pub retry: Option<crate::module::RetryConfig>,
    pub timeout_ms: u64,
    pub depends_on: Vec<String>,
    pub parallel_group: Option<String>,
    pub condition: Option<String>,
    pub error_handler: Option<Box<ModuleNode>>,
}

/// A materialized run DAG — compiled from the design graph for one execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunGraph {
    pub id: String,
    pub workflow_name: String,
    pub modules: Vec<ModuleNode>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Execution state for a running module
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionState {
    Pending,
    Running,
    Completed(serde_json::Value),
    Failed(String),
    Skipped(String),
    Retrying(u32),
}
