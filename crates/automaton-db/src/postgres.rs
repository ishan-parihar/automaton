//! Postgres backend for DbPool using tokio-postgres + deadpool-postgres.
//! No SQLite dependency — avoids the libsqlite3-sys native link conflict.

use async_trait::async_trait;
use chrono::Utc;
use deadpool_postgres::{Config, Pool, RecyclingMethod, Runtime};
use serde_json::Value;
use tokio_postgres::{NoTls, Row};

use crate::models::*;
use crate::DbPool;

/// Postgres-backed DbPool with deadpool connection management
pub struct PostgresPool {
    pool: Pool,
}

impl PostgresPool {
    /// Connect to Postgres and run migrations.
    /// Connection string: "host=localhost user=automaton dbname=automaton"
    pub async fn connect(database_url: &str) -> Result<Self, String> {
        let config: Config = database_url.parse().map_err(|e| format!("Invalid DB URL: {e}"))?;
        let pool = config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| format!("Pool creation failed: {e}"))?;

        let pg = Self { pool };
        pg.migrate().await?;
        Ok(pg)
    }

    async fn migrate(&self) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool get: {e}"))?;

        let ddl = vec![
            "CREATE TABLE IF NOT EXISTS scripts (
                hash TEXT PRIMARY KEY, path TEXT NOT NULL, version TEXT NOT NULL DEFAULT '0.1.0',
                parent_hash TEXT REFERENCES scripts(hash), source TEXT NOT NULL,
                manifest JSONB NOT NULL DEFAULT '{}', built BOOLEAN NOT NULL DEFAULT false,
                language TEXT NOT NULL DEFAULT 'rust', folder_id TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS script_deps (
                script_hash TEXT NOT NULL REFERENCES scripts(hash) ON DELETE CASCADE,
                depends_on TEXT NOT NULL, version_req TEXT,
                PRIMARY KEY (script_hash, depends_on)
            )",
            "CREATE TABLE IF NOT EXISTS flows (
                hash TEXT PRIMARY KEY, path TEXT NOT NULL, version TEXT NOT NULL DEFAULT '0.1.0',
                parent_hash TEXT REFERENCES flows(hash), definition JSONB NOT NULL,
                folder_id TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS jobs (
                id BIGSERIAL PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'script',
                target_path TEXT NOT NULL, args JSONB NOT NULL DEFAULT '{}',
                scheduled_for TIMESTAMPTZ NOT NULL DEFAULT NOW(), priority INT NOT NULL DEFAULT 0,
                tag TEXT, running BOOLEAN NOT NULL DEFAULT false, worker_id TEXT,
                max_attempts INT NOT NULL DEFAULT 3, attempt INT NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY, target_path TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'script',
                args JSONB NOT NULL DEFAULT '{}', result JSONB, error TEXT,
                state TEXT NOT NULL DEFAULT 'pending', attempt INT NOT NULL DEFAULT 1,
                duration_ms BIGINT NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ
            )",
            "CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL,
                properties JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS graph_edges (
                id TEXT PRIMARY KEY, source TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                target TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                kind TEXT NOT NULL, properties JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS variables (
                path TEXT PRIMARY KEY, value TEXT NOT NULL,
                is_secret BOOLEAN NOT NULL DEFAULT true, description TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS resources (
                path TEXT PRIMARY KEY, resource_type TEXT NOT NULL,
                value JSONB NOT NULL DEFAULT '{}', description TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS triggers (
                id TEXT PRIMARY KEY, target_path TEXT NOT NULL,
                target_is_flow BOOLEAN NOT NULL DEFAULT false,
                trigger_type TEXT NOT NULL DEFAULT 'cron', config JSONB NOT NULL DEFAULT '{}',
                enabled BOOLEAN NOT NULL DEFAULT true, last_fired_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS build_cache (
                hash TEXT PRIMARY KEY, script_path TEXT NOT NULL, language TEXT NOT NULL,
                binary_path TEXT NOT NULL, binary_size BIGINT, build_duration_ms BIGINT,
                build_mode TEXT NOT NULL DEFAULT 'debug', success BOOLEAN NOT NULL DEFAULT true,
                error_log TEXT, built_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE INDEX IF NOT EXISTS idx_scripts_path ON scripts(path, created_at DESC)",
            "CREATE INDEX IF NOT EXISTS idx_jobs_pending ON jobs(scheduled_for, priority DESC) WHERE NOT running",
            "CREATE INDEX IF NOT EXISTS idx_runs_target ON runs(target_path, created_at DESC)",
            "CREATE INDEX IF NOT EXISTS idx_nodes_kind ON graph_nodes(kind)",
            "CREATE INDEX IF NOT EXISTS idx_edges_source ON graph_edges(source)",
            "CREATE INDEX IF NOT EXISTS idx_triggers_enabled ON triggers(enabled, trigger_type)",
        ];

        for stmt in ddl {
            client.batch_execute(stmt).await.map_err(|e| format!("Migration: {e}"))?;
        }
        Ok(())
    }

    fn row_to_script(row: &Row) -> Result<ScriptRecord, String> {
        Ok(ScriptRecord {
            hash: row.try_get("hash").map_err(|e| format!("hash: {e}"))?,
            path: row.try_get("path").map_err(|e| format!("path: {e}"))?,
            version: row.try_get("version").map_err(|e| format!("version: {e}"))?,
            source: row.try_get("source").map_err(|e| format!("source: {e}"))?,
            manifest: row.try_get::<_, Value>("manifest").map_err(|e| format!("manifest: {e}"))?,
            built: row.try_get("built").map_err(|e| format!("built: {e}"))?,
            created_at: row.try_get::<_, chrono::DateTime<Utc>>("created_at")
                .map(|t| t.to_rfc3339()).map_err(|e| format!("created_at: {e}"))?,
        })
    }
}

#[async_trait]
impl DbPool for PostgresPool {
    // ── Scripts ──
    async fn register_script(
        &self,
        path: &str,
        source: &str,
        version: &str,
        manifest: &Value,
        deps: &[automaton_core::DepRef],
    ) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "INSERT INTO scripts (hash, path, version, source, manifest)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (hash) DO UPDATE SET path = EXCLUDED.path",
                &[&hash, &path, &version, &source, &manifest],
            )
            .await
            .map_err(|e| format!("Insert script: {e}"))?;
        for dep in deps {
            client
                .execute(
                    "INSERT INTO script_deps (script_hash, depends_on, version_req)
                     VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                    &[&hash, &dep.name, &dep.version_req],
                )
                .await
                .map_err(|e| format!("Insert dep: {e}"))?;
        }
        Ok(hash)
    }

    async fn get_script(&self, path: &str) -> Result<Option<ScriptRecord>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query(
                "SELECT hash, path, version, source, manifest, built, created_at
                 FROM scripts WHERE path = $1 ORDER BY created_at DESC LIMIT 1",
                &[&path],
            )
            .await
            .map_err(|e| format!("Query: {e}"))?;
        Ok(rows.first().map(|r| Self::row_to_script(r).unwrap()))
    }

    async fn list_scripts(&self) -> Result<Vec<ScriptRecord>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query(
                "SELECT DISTINCT ON (path) hash, path, version, source, manifest, built, created_at
                 FROM scripts ORDER BY path, created_at DESC",
                &[],
            )
            .await
            .map_err(|e| format!("Query: {e}"))?;
        rows.iter().map(|r| Self::row_to_script(r)).collect()
    }

    async fn mark_built(&self, path: &str) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute("UPDATE scripts SET built = true WHERE path = $1", &[&path])
            .await
            .map_err(|e| format!("Update: {e}"))?;
        Ok(())
    }

    // ── Jobs ──
    async fn enqueue(&self, kind: &str, target: &str, args: &Value) -> Result<i64, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let row = client
            .query_one(
                "INSERT INTO jobs (kind, target_path, args) VALUES ($1, $2, $3) RETURNING id",
                &[&kind, &target, &args],
            )
            .await
            .map_err(|e| format!("Insert: {e}"))?;
        Ok(row.get("id"))
    }

    async fn dequeue(&self, worker_id: &str) -> Result<Option<JobRecord>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let row = client
            .query_opt(
                "UPDATE jobs SET running = true, worker_id = $1, attempt = attempt + 1
                 WHERE id = (
                     SELECT id FROM jobs
                     WHERE NOT running AND scheduled_for <= NOW()
                     ORDER BY priority DESC, scheduled_for ASC
                     LIMIT 1
                     FOR UPDATE SKIP LOCKED
                 )
                 RETURNING id, kind, target_path, args, scheduled_for, priority",
                &[&worker_id],
            )
            .await
            .map_err(|e| format!("Dequeue: {e}"))?;
        Ok(row.map(|r| JobRecord {
            id: r.get("id"),
            kind: r.get("kind"),
            target_path: r.get("target_path"),
            args: r.get("args"),
            scheduled_for: r.get::<_, chrono::DateTime<Utc>>("scheduled_for").to_rfc3339(),
            priority: r.get("priority"),
        }))
    }

    async fn complete_job(&self, job_id: i64) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute("DELETE FROM jobs WHERE id = $1", &[&job_id])
            .await
            .map_err(|e| format!("Delete: {e}"))?;
        Ok(())
    }

    // ── Runs ──
    async fn record_run(&self, id: &str, target: &str, kind: &str, args: &Value) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "INSERT INTO runs (id, target_path, kind, args) VALUES ($1, $2, $3, $4)",
                &[&id, &target, &kind, &args],
            )
            .await
            .map_err(|e| format!("Insert run: {e}"))?;
        Ok(())
    }

    async fn update_run(
        &self,
        id: &str,
        state: &str,
        result: Option<&Value>,
        error: Option<&str>,
        attempt: u32,
        duration_ms: i64,
    ) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "UPDATE runs SET state = $1, result = $2, error = $3, attempt = $4,
                 duration_ms = $5, completed_at = NOW() WHERE id = $6",
                &[&state, &result, &error, &(attempt as i32), &duration_ms, &id],
            )
            .await
            .map_err(|e| format!("Update run: {e}"))?;
        Ok(())
    }

    async fn get_runs(&self, target: &str, limit: i64) -> Result<Vec<RunRecord>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query(
                "SELECT id, target_path, state, attempt, error, duration_ms, created_at
                 FROM runs WHERE target_path = $1 ORDER BY created_at DESC LIMIT $2",
                &[&target, &limit],
            )
            .await
            .map_err(|e| format!("Query runs: {e}"))?;
        Ok(rows.iter().map(|r| RunRecord {
            id: r.get("id"),
            target_path: r.get("target_path"),
            state: r.get("state"),
            attempt: r.get("attempt"),
            error: r.get("error"),
            duration_ms: r.get("duration_ms"),
            created_at: r.get::<_, chrono::DateTime<Utc>>("created_at").to_rfc3339(),
        }).collect())
    }

    // ── Graph ──
    async fn add_node(&self, kind: &str, name: &str, props: &Value) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "INSERT INTO graph_nodes (id, kind, name, properties) VALUES ($1, $2, $3, $4)",
                &[&id, &kind, &name, &props],
            )
            .await
            .map_err(|e| format!("Insert node: {e}"))?;
        Ok(id)
    }

    async fn add_edge(&self, source: &str, target: &str, kind: &str) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "INSERT INTO graph_edges (id, source, target, kind) VALUES ($1, $2, $3, $4)",
                &[&id, &source, &target, &kind],
            )
            .await
            .map_err(|e| format!("Insert edge: {e}"))?;
        Ok(id)
    }

    async fn get_nodes(&self, kind: Option<&str>) -> Result<Vec<NodeRecord>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = if let Some(k) = kind {
            client
                .query(
                    "SELECT id, kind, name, properties, created_at FROM graph_nodes WHERE kind = $1 ORDER BY created_at",
                    &[&k],
                )
                .await.map_err(|e| format!("Query: {e}"))?
        } else {
            client
                .query("SELECT id, kind, name, properties, created_at FROM graph_nodes ORDER BY created_at", &[])
                .await.map_err(|e| format!("Query: {e}"))?
        };
        Ok(rows.iter().map(|r| NodeRecord {
            id: r.get("id"),
            kind: r.get("kind"),
            name: r.get("name"),
            properties: r.get("properties"),
            created_at: r.get::<_, chrono::DateTime<Utc>>("created_at").to_rfc3339(),
        }).collect())
    }

    async fn get_edges(&self) -> Result<Vec<EdgeRecord>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query("SELECT id, source, target, kind, properties, created_at FROM graph_edges ORDER BY created_at", &[])
            .await.map_err(|e| format!("Query: {e}"))?;
        Ok(rows.iter().map(|r| EdgeRecord {
            id: r.get("id"),
            source: r.get("source"),
            target: r.get("target"),
            kind: r.get("kind"),
            properties: r.get("properties"),
            created_at: r.get::<_, chrono::DateTime<Utc>>("created_at").to_rfc3339(),
        }).collect())
    }

    // ── Secrets ──
    async fn set_variable(&self, path: &str, value: &str, is_secret: bool) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "INSERT INTO variables (path, value, is_secret)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (path) DO UPDATE SET value = EXCLUDED.value, is_secret = EXCLUDED.is_secret",
                &[&path, &value, &is_secret],
            )
            .await
            .map_err(|e| format!("Insert variable: {e}"))?;
        Ok(())
    }

    async fn get_variable(&self, path: &str) -> Result<Option<String>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query("SELECT value FROM variables WHERE path = $1", &[&path])
            .await
            .map_err(|e| format!("Query: {e}"))?;
        Ok(rows.first().map(|r| r.get("value")))
    }

    async fn list_variables(&self) -> Result<Vec<Value>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query("SELECT path, is_secret, description FROM variables ORDER BY path", &[])
            .await
            .map_err(|e| format!("Query: {e}"))?;
        Ok(rows.iter().map(|r| serde_json::json!({
            "path": r.get::<_, String>("path"),
            "is_secret": r.get::<_, bool>("is_secret"),
            "description": r.get::<_, Option<String>>("description"),
        })).collect())
    }

    // ── Resources ──
    async fn set_resource(&self, path: &str, rtype: &str, value: &Value) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "INSERT INTO resources (path, resource_type, value)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (path) DO UPDATE SET resource_type = EXCLUDED.resource_type, value = EXCLUDED.value",
                &[&path, &rtype, &value],
            )
            .await
            .map_err(|e| format!("Insert resource: {e}"))?;
        Ok(())
    }

    async fn get_resource(&self, path: &str) -> Result<Option<Value>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query("SELECT resource_type, value FROM resources WHERE path = $1", &[&path])
            .await
            .map_err(|e| format!("Query: {e}"))?;
        Ok(rows.first().map(|r| serde_json::json!({
            "type": r.get::<_, String>("resource_type"),
            "value": r.get::<_, Value>("value"),
        })))
    }

    async fn list_resources(&self, rtype: Option<&str>) -> Result<Vec<Value>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = if let Some(t) = rtype {
            client
                .query("SELECT path, resource_type FROM resources WHERE resource_type = $1 ORDER BY path", &[&t])
                .await.map_err(|e| format!("Query: {e}"))?
        } else {
            client
                .query("SELECT path, resource_type FROM resources ORDER BY path", &[])
                .await.map_err(|e| format!("Query: {e}"))?
        };
        Ok(rows.iter().map(|r| serde_json::json!({
            "path": r.get::<_, String>("path"),
            "type": r.get::<_, String>("resource_type"),
        })).collect())
    }

    // ── Triggers ──
    async fn create_trigger(&self, target: &str, is_flow: bool, ttype: &str, config: &Value) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        client
            .execute(
                "INSERT INTO triggers (id, target_path, target_is_flow, trigger_type, config)
                 VALUES ($1, $2, $3, $4, $5)",
                &[&id, &target, &is_flow, &ttype, &config],
            )
            .await
            .map_err(|e| format!("Insert trigger: {e}"))?;
        Ok(id)
    }

    async fn get_enabled_triggers(&self, ttype: &str) -> Result<Vec<TriggerRecord>, String> {
        let client = self.pool.get().await.map_err(|e| format!("Pool: {e}"))?;
        let rows = client
            .query(
                "SELECT id, target_path, target_is_flow, config, created_at
                 FROM triggers WHERE enabled AND trigger_type = $1",
                &[&ttype],
            )
            .await
            .map_err(|e| format!("Query: {e}"))?;
        Ok(rows.iter().map(|r| TriggerRecord {
            id: r.get("id"),
            target_path: r.get("target_path"),
            target_is_flow: r.get("target_is_flow"),
            config: r.get("config"),
            created_at: r.get::<_, chrono::DateTime<Utc>>("created_at").to_rfc3339(),
        }).collect())
    }
}
