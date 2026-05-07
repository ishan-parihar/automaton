//! Trait-based database backend abstraction for Automaton.
//! Enables the engine to work with either SQLite (via Registry) or Postgres (via AutomatonDb).

use crate::*;

/// Abstraction over database backends for the automaton engine.
/// Both `automaton_registry::Registry` and `automaton_postgres::AutomatonDb` implement this.
#[async_trait::async_trait]
pub trait RegistryBackend: Send + Sync {
    // ── Module management ──

    async fn register_module(&self, path: &str, source: &str, manifest: &AutomationManifest) -> Result<ModuleId>;

    async fn get_module(&self, path: &str) -> Result<Option<AutomationModule>>;

    async fn list_modules(&self) -> Result<Vec<(String, String, String, bool)>>;

    async fn mark_built(&self, path: &str) -> Result<()>;

    async fn record_build(&self, hash: &str, artifact_path: &str, mode: &str) -> Result<()>;

    /// Record a module execution run
    async fn record_run(&self, run_id: &str, module_path: &str, input: &serde_json::Value) -> Result<()>;

    /// Update run status after execution
    async fn update_run(
        &self,
        run_id: &str,
        status: &str,
        output: Option<&serde_json::Value>,
        error_msg: Option<&str>,
        attempt: u32,
    ) -> Result<()>;

    async fn get_runs(&self, module_path: &str) -> Result<Vec<serde_json::Value>>;

    // ── Build cache directory ──

    fn build_cache_dir(&self) -> std::path::PathBuf;

    // ── Variable/resource resolution ──

    async fn resolve_references(&self, val: &serde_json::Value) -> Result<serde_json::Value>;

    // ── Job queue ──

    async fn enqueue_job(&self, kind: &str, target: &str, args: &serde_json::Value) -> Result<i64>;

    async fn dequeue_job(&self, worker_id: &str) -> Result<Option<serde_json::Value>>;

    async fn complete_job(&self, job_id: i64) -> Result<()>;

    async fn list_jobs(&self, limit: i64) -> Result<Vec<serde_json::Value>>;

    // ── Triggers ──

    async fn create_trigger(&self, target: &str, is_flow: bool, ttype: &str, config: &serde_json::Value) -> Result<String>;

    async fn get_enabled_triggers(&self, ttype: &str) -> Result<Vec<serde_json::Value>>;

    // ── Variables ──

    async fn get_variable(&self, path: &str) -> Result<Option<String>>;

    async fn set_variable(&self, path: &str, value: &str, is_secret: bool) -> Result<()>;

    async fn list_variables(&self) -> Result<Vec<serde_json::Value>>;

    // ── Resources ──

    async fn get_resource(&self, path: &str) -> Result<Option<serde_json::Value>>;

    async fn set_resource(&self, path: &str, resource_type: &str, value: &serde_json::Value) -> Result<()>;

    async fn list_resources(&self, resource_type: Option<&str>) -> Result<Vec<serde_json::Value>>;

    // ── Flows ──

    async fn store_flow(&self, path: &str, version: &str, definition: &serde_json::Value, summary: Option<&str>, on_failure: Option<&str>) -> Result<String>;

    async fn get_flow(&self, path: &str) -> Result<Option<serde_json::Value>>;

    async fn list_flows(&self) -> Result<Vec<serde_json::Value>>;

    async fn delete_flow(&self, path: &str) -> Result<()>;

    // ── Webhook management ──

    async fn register_webhook(&self, target_url: &str, event: &str, secret: Option<&str>) -> Result<String>;

    async fn list_webhooks(&self, event: Option<&str>) -> Result<Vec<serde_json::Value>>;

    async fn delete_webhook(&self, id: &str) -> Result<()>;

    // ── Execution history ──

    async fn store_execution(&self, execution: &FlowExecution) -> Result<()>;

    async fn list_executions(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>>;
}
