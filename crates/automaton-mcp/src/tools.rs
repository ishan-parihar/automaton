use serde_json::Value;
use std::sync::Arc;

use schemars::JsonSchema;

/// Generate a JSON Schema from a JsonSchema-derived type
pub fn schema_for<T: JsonSchema>() -> Arc<serde_json::Map<String, serde_json::Value>> {
    let schema = schemars::schema_for!(T);
    let value = serde_json::to_value(&schema).unwrap_or_default();
    match value {
        Value::Object(map) => Arc::new(map),
        _ => Arc::new(serde_json::Map::new()),
    }
}

// ── Module Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct ModuleCreateParams {
    pub path: String,
    pub source: String,
    pub version: Option<String>,
    pub summary: Option<String>,
    pub depends_on: Option<Vec<String>>,
    pub timeout_ms: Option<u64>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct ModuleBuildParams {
    pub path: String,
    pub mode: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct ModuleRunParams {
    pub path: String,
    pub input: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct ModuleDeprecateParams {
    pub path: String,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct ModuleSearchParams {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct ModuleTemplateParams {
    pub path: String,
    pub pattern: String,
    pub description: Option<String>,
}

// ── Workflow Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct WorkflowPlanParams {
    pub start: String,
    pub max_depth: Option<usize>,
}

// ── Graph Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct GraphQueryParams {
    pub kind: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct GraphPathfindParams {
    pub from: String,
    pub to: String,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct GraphAddEdgeParams {
    pub source: String,
    pub target: String,
    pub kind: String,
}

// ── Flow Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct FlowCreateParams {
    pub path: String,
    pub steps: serde_json::Value,
    pub summary: Option<String>,
    pub on_failure: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct FlowShowParams {
    pub path: String,
}

// ── Schedule Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct ScheduleCreateParams {
    pub target_path: String,
    pub schedule: String,
    pub args: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct ScheduleValidateParams {
    pub schedule: String,
}

// ── Secret Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct SecretSetParams {
    pub path: String,
    pub value: String,
    pub description: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
pub struct SecretGetParams {
    pub path: String,
}

// ── Resource Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct ResourceBindParams {
    pub path: String,
    pub resource_type: String,
    pub value: serde_json::Value,
}

// ── Job Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct JobQueueParams {
    pub target_path: String,
    pub args: Option<serde_json::Value>,
    pub kind: Option<String>,
}

// ── Run Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct RunLogsParams {
    pub module_path: Option<String>,
    pub limit: Option<usize>,
}

// ── Registry Tools ──
#[derive(serde::Deserialize, JsonSchema)]
pub struct RegistrySearchParams {
    pub query: String,
}
