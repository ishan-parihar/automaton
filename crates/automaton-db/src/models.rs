//! Shared record types for DbPool operations

use serde::{Deserialize, Serialize};

/// A script record from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRecord {
    pub hash: String,
    pub path: String,
    pub version: String,
    pub source: String,
    pub manifest: serde_json::Value,
    pub built: bool,
    pub created_at: String,
}

/// A job record from the queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: i64,
    pub kind: String,
    pub target_path: String,
    pub args: serde_json::Value,
    pub scheduled_for: String,
    pub priority: i32,
}

/// A run record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub target_path: String,
    pub state: String,
    pub attempt: i32,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub created_at: String,
}

/// A graph node record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub properties: serde_json::Value,
    pub created_at: String,
}

/// A graph edge record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub properties: serde_json::Value,
    pub created_at: String,
}

/// A trigger record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerRecord {
    pub id: String,
    pub target_path: String,
    pub target_is_flow: bool,
    pub config: serde_json::Value,
    pub created_at: String,
}
