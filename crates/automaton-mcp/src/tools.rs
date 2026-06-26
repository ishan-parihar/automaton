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
    #[schemars(title = "Path", description = "Dot-separated module path, e.g. github.issue_triage")]
    pub path: String,
    #[schemars(title = "Source", description = "Rust source code for the module")]
    pub source: String,
    #[schemars(title = "Version", description = "Semver version string (default: 0.1.0)")]
    pub version: Option<String>,
    #[schemars(title = "Summary", description = "One-line description of what the module does")]
    pub summary: Option<String>,
    #[schemars(title = "Dependencies", description = "List of module paths this module depends on")]
    pub depends_on: Option<Vec<String>>,
    #[schemars(title = "Timeout", description = "Execution timeout in milliseconds (default: 30000)")]
    pub timeout_ms: Option<u64>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleBuildParams {
    #[schemars(title = "Path", description = "Dot-separated module path to build")]
    pub path: String,
    #[schemars(title = "Mode", description = "Build mode: \"release\" or \"debug\" (default: release)")]
    pub mode: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleRunParams {
    #[schemars(title = "Path", description = "Dot-separated module path to execute")]
    pub path: String,
    #[schemars(title = "Input", description = "JSON input payload for the module")]
    pub input: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleDeprecateParams {
    #[schemars(title = "Path", description = "Dot-separated module path to deprecate")]
    pub path: String,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleSearchParams {
    #[schemars(title = "Query", description = "Substring to match against module paths")]
    pub query: String,
    #[schemars(title = "Limit", description = "Maximum results to return (default: 20)")]
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleTemplateParams {
    #[schemars(title = "Path", description = "Dot-separated module path for the new module")]
    pub path: String,
    #[schemars(title = "Pattern", description = "Template pattern name (e.g. \"echo\", \"http\", \"cron\")")]
    pub pattern: String,
    #[schemars(title = "Description", description = "One-line description of the module")]
    pub description: Option<String>,
}

// ── Workflow Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlanParams {
    #[schemars(title = "Start", description = "Starting module path for workflow planning")]
    pub start: String,
    #[schemars(title = "Max Depth", description = "Maximum dependency traversal depth (default: 10)")]
    pub max_depth: Option<usize>,
}

// ── Graph Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphQueryParams {
    #[schemars(title = "Kind", description = "Filter by node kind: module, workflow, trigger, resource, run")]
    pub kind: Option<String>,
    #[schemars(title = "Limit", description = "Maximum nodes to return")]
    pub limit: Option<u32>,
    #[schemars(title = "Offset", description = "Pagination offset")]
    pub offset: Option<u32>,
    #[schemars(title = "Properties", description = "Filter by JSON property key-value pairs")]
    pub properties: Option<HashMap<String, Value>>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphPathfindParams {
    #[schemars(title = "From", description = "Source node ID or name")]
    pub from: String,
    #[schemars(title = "To", description = "Target node ID or name")]
    pub to: String,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphAddEdgeParams {
    #[schemars(title = "Source", description = "Source node ID")]
    pub source: String,
    #[schemars(title = "Target", description = "Target node ID")]
    pub target: String,
    #[schemars(title = "Kind", description = "Edge kind: DEPENDS_ON, CALLS, TRIGGERS, USES_RESOURCE, EMITS, CONSUMES, BLOCKED_BY, ALTERNATIVE_TO, UPGRADES, DERIVED_FROM")]
    pub kind: String,
}

// ── Flow Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowCreateParams {
    #[schemars(title = "Path", description = "Dot-separated flow path, e.g. deploy.pipeline")]
    pub path: String,
    #[schemars(title = "Steps", description = "JSON array of flow step definitions")]
    pub steps: serde_json::Value,
    #[schemars(title = "Summary", description = "One-line description of the flow")]
    pub summary: Option<String>,
    #[schemars(title = "On Failure", description = "Failure strategy: \"abort\", \"skip\", or \"retry\"")]
    pub on_failure: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowShowParams {
    #[schemars(title = "Path", description = "Dot-separated flow path to inspect")]
    pub path: String,
}

// ── Schedule Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleCreateParams {
    #[schemars(title = "Target Path", description = "Module or flow path to schedule")]
    pub target_path: String,
    #[schemars(title = "Schedule", description = "Cron expression, e.g. \"0 */6 * * *\"")]
    pub schedule: String,
    #[schemars(title = "Args", description = "Optional JSON arguments passed to the target on each run")]
    pub args: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleValidateParams {
    #[schemars(title = "Schedule", description = "Cron expression to validate")]
    pub schedule: String,
}

// ── Secret Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretSetParams {
    #[schemars(title = "Path", description = "Dot-separated secret path, e.g. github.token")]
    pub path: String,
    #[schemars(title = "Value", description = "Secret value to store (encrypted at rest)")]
    pub value: String,
    #[schemars(title = "Description", description = "Optional human-readable note about this secret")]
    pub description: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretGetParams {
    #[schemars(title = "Path", description = "Dot-separated secret path to retrieve")]
    pub path: String,
}

// ── Resource Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceBindParams {
    #[schemars(title = "Path", description = "Dot-separated resource path, e.g. db.main")]
    pub path: String,
    #[schemars(title = "Resource Type", description = "Resource type: postgresql, slack, github, openai, http, aws")]
    pub resource_type: String,
    #[schemars(title = "Value", description = "JSON config for the resource (connection string, API key, etc.)")]
    pub value: serde_json::Value,
}

// ── Job Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobQueueParams {
    #[schemars(title = "Target Path", description = "Module or flow path to enqueue")]
    pub target_path: String,
    #[schemars(title = "Args", description = "Optional JSON arguments for the job")]
    pub args: Option<serde_json::Value>,
    #[schemars(title = "Kind", description = "Job kind: \"script\", \"flow\", or \"module\" (default: script)")]
    pub kind: Option<String>,
}

// ── Flow Tools (continued) ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowExecuteParams {
    #[schemars(title = "Path", description = "Dot-separated flow path to execute")]
    pub path: String,
    #[schemars(title = "Input", description = "JSON input payload passed to the flow")]
    pub input: Option<serde_json::Value>,
}

// ── Run Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunLogsParams {
    #[schemars(title = "Module Path", description = "Filter runs by module path (omit for all)")]
    pub module_path: Option<String>,
    #[schemars(title = "Limit", description = "Maximum runs to return (default: 20)")]
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRetryParams {
    #[schemars(title = "Run ID", description = "ID of the failed run to retry")]
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
    #[schemars(title = "Query", description = "Substring to match against registered module paths")]
    pub query: String,
}

// ── Graph Search Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    #[schemars(title = "Query", description = "Text query to search graph node names and properties")]
    pub query: String,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeRangeParams {
    #[schemars(title = "Start", description = "ISO 8601 start timestamp")]
    pub start: String,
    #[schemars(title = "End", description = "ISO 8601 end timestamp")]
    pub end: String,
    #[schemars(title = "Kind", description = "Filter by node kind: module, workflow, trigger, resource, run")]
    pub kind: Option<String>,
}

// ── Webhook Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookRegisterParams {
    #[schemars(title = "URL", description = "HTTPS endpoint to receive webhook POST requests")]
    pub url: String,
    #[schemars(title = "Event", description = "Event type to subscribe to, e.g. \"module.built\", \"flow.completed\"")]
    pub event: String,
    #[schemars(title = "Secret", description = "Optional shared secret for HMAC signature verification")]
    pub secret: Option<String>,
}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookListParams {}

#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookDeleteParams {
    #[schemars(title = "ID", description = "Webhook registration ID to delete")]
    pub id: String,
}

// ── Flow Telemetry Tools ──
#[derive(serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowExecuteTelemetryParams {
    #[schemars(title = "Path", description = "Dot-separated flow path to execute with telemetry")]
    pub path: String,
    #[schemars(title = "Input", description = "JSON input payload passed to the flow")]
    pub input: Option<serde_json::Value>,
    #[schemars(title = "Progress Token", description = "MCP progress token for receiving step-by-step notifications")]
    pub progress_token: Option<String>,
}
