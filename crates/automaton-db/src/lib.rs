//! Database abstraction layer for Automaton.
//!
//! Provides a unified `DbPool` trait with two backends:
//! - `sqlite` (default): rusqlite-backed, for local development
//! - `postgres`: tokio-postgres + deadpool-postgres, for production
//!
//! Both are mutually exclusive via cargo features — never simultaneously compiled.

pub mod models;

use async_trait::async_trait;
use models::*;

/// Unified database interface for all Automaton operations.
/// Each backend (SQLite, Postgres) implements this trait.
#[async_trait]
pub trait DbPool: Send + Sync {
    // ── Scripts ──
    async fn register_script(
        &self,
        path: &str,
        source: &str,
        version: &str,
        manifest: &serde_json::Value,
        deps: &[automaton_core::DepRef],
    ) -> Result<String, String>;
    async fn get_script(&self, path: &str) -> Result<Option<ScriptRecord>, String>;
    async fn list_scripts(&self) -> Result<Vec<ScriptRecord>, String>;
    async fn mark_built(&self, path: &str) -> Result<(), String>;

    // ── Jobs ──
    async fn enqueue(
        &self,
        kind: &str,
        target: &str,
        args: &serde_json::Value,
    ) -> Result<i64, String>;
    async fn dequeue(&self, worker_id: &str) -> Result<Option<JobRecord>, String>;
    async fn complete_job(&self, job_id: i64) -> Result<(), String>;

    // ── Runs ──
    async fn record_run(
        &self,
        id: &str,
        target: &str,
        kind: &str,
        args: &serde_json::Value,
    ) -> Result<(), String>;
    async fn update_run(
        &self,
        id: &str,
        state: &str,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
        attempt: u32,
        duration_ms: i64,
    ) -> Result<(), String>;
    async fn get_runs(&self, target: &str, limit: i64) -> Result<Vec<RunRecord>, String>;

    // ── Graph ──
    async fn add_node(
        &self,
        kind: &str,
        name: &str,
        props: &serde_json::Value,
    ) -> Result<String, String>;
    async fn add_edge(
        &self,
        source: &str,
        target: &str,
        kind: &str,
    ) -> Result<String, String>;
    async fn get_nodes(
        &self,
        kind: Option<&str>,
    ) -> Result<Vec<NodeRecord>, String>;
    async fn get_edges(&self) -> Result<Vec<EdgeRecord>, String>;

    // ── Secrets ──
    async fn set_variable(
        &self,
        path: &str,
        value: &str,
        is_secret: bool,
    ) -> Result<(), String>;
    async fn get_variable(&self, path: &str) -> Result<Option<String>, String>;
    async fn list_variables(&self) -> Result<Vec<serde_json::Value>, String>;

    // ── Resources ──
    async fn set_resource(
        &self,
        path: &str,
        rtype: &str,
        value: &serde_json::Value,
    ) -> Result<(), String>;
    async fn get_resource(
        &self,
        path: &str,
    ) -> Result<Option<serde_json::Value>, String>;
    async fn list_resources(
        &self,
        rtype: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, String>;

    // ── Triggers ──
    async fn create_trigger(
        &self,
        target: &str,
        is_flow: bool,
        ttype: &str,
        config: &serde_json::Value,
    ) -> Result<String, String>;
    async fn get_enabled_triggers(
        &self,
        ttype: &str,
    ) -> Result<Vec<TriggerRecord>, String>;
}

// ── Feature-gated backend selection ──

#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "postgres")]
pub use postgres::PostgresPool as DefaultPool;

#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "sqlite")]
pub use sqlite::SqlitePool as DefaultPool;
