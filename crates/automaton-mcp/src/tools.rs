use serde_json::Value;
use std::collections::HashMap;
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
#[serde(deny_unknown_fields)]
pub struct ModuleCreateParams {
    pub path: String,
    pub source: String,
    pub version: Option<String>,
    pub summary: Option<String>,
    pub depends_on: Option<Vec<String>>,
    pub timeout_ms: Option<u64>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleBuildParams {
    pub path: String,
    pub mode: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleRunParams {
    pub path: String,
    pub input: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleDeprecateParams {
    pub path: String,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleSearchParams {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleTemplateParams {
    pub path: String,
    pub pattern: String,
    pub description: Option<String>,
}

// ── Workflow Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlanParams {
    pub start: String,
    pub max_depth: Option<usize>,
}

// ── Graph Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphQueryParams {
    pub kind: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub properties: Option<HashMap<String, Value>>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphPathfindParams {
    pub from: String,
    pub to: String,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphAddEdgeParams {
    pub source: String,
    pub target: String,
    pub kind: String,
}

// ── Flow Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowCreateParams {
    pub path: String,
    pub steps: serde_json::Value,
    pub summary: Option<String>,
    pub on_failure: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowShowParams {
    pub path: String,
}

// ── Schedule Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleCreateParams {
    pub target_path: String,
    pub schedule: String,
    pub args: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleValidateParams {
    pub schedule: String,
}

// ── Secret Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretSetParams {
    pub path: String,
    pub value: String,
    pub description: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretGetParams {
    pub path: String,
}

// ── Resource Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceBindParams {
    pub path: String,
    pub resource_type: String,
    pub value: serde_json::Value,
}

// ── Job Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobQueueParams {
    pub target_path: String,
    pub args: Option<serde_json::Value>,
    pub kind: Option<String>,
}

// ── Flow Tools (continued) ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowExecuteParams {
    pub path: String,
    pub input: Option<serde_json::Value>,
}

// ── Run Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunLogsParams {
    pub module_path: Option<String>,
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRetryParams {
    pub run_id: String,
}

// ── Graph Tools (continued) ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphSummarizeParams {}

// ── Registry Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistrySearchParams {
    pub query: String,
}

// ── Graph Search Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    pub query: String,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeRangeParams {
    pub start: String,
    pub end: String,
    pub kind: Option<String>,
}

// ── Webhook Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookRegisterParams {
    pub url: String,
    pub event: String,
    pub secret: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookListParams {}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookDeleteParams {
    pub id: String,
}

// ── Flow Telemetry Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowExecuteTelemetryParams {
    pub path: String,
    pub input: Option<serde_json::Value>,
    pub progress_token: Option<String>,
}
