//! Postgres database layer for Automaton.
//! All SQL operations go through this crate — modules, flows, jobs, secrets, triggers, graph.

use automaton_core::FlowExecution;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;

pub struct AutomatonDb {
    pool: PgPool,
}

impl AutomatonDb {
    /// Connect to Postgres and run migrations.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await?;
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS scripts (
                hash TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                version TEXT NOT NULL DEFAULT '0.1.0',
                source TEXT NOT NULL,
                manifest JSONB NOT NULL DEFAULT '{}',
                parent_hash TEXT REFERENCES scripts(hash),
                built BOOLEAN NOT NULL DEFAULT false,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS script_deps (
                script_hash TEXT NOT NULL REFERENCES scripts(hash) ON DELETE CASCADE,
                depends_on TEXT NOT NULL,
                version_req TEXT,
                PRIMARY KEY (script_hash, depends_on)
            );
            CREATE TABLE IF NOT EXISTS flows (
                hash TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                version TEXT NOT NULL DEFAULT '0.1.0',
                flow_definition JSONB NOT NULL,
                parent_hash TEXT REFERENCES flows(hash),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS jobs (
                id BIGSERIAL PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'script',
                target_path TEXT NOT NULL,
                args JSONB NOT NULL DEFAULT '{}',
                scheduled_for TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                priority INT NOT NULL DEFAULT 0,
                running BOOLEAN NOT NULL DEFAULT false,
                worker_id TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                target_path TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'script',
                args JSONB NOT NULL DEFAULT '{}',
                result JSONB,
                error TEXT,
                state TEXT NOT NULL DEFAULT 'pending',
                attempt INT NOT NULL DEFAULT 1,
                duration_ms BIGINT NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                completed_at TIMESTAMPTZ
            );
            CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                properties JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS graph_edges (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                target TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                properties JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS variables (
                path TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                is_secret BOOLEAN NOT NULL DEFAULT true,
                description TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS resources (
                path TEXT PRIMARY KEY,
                resource_type TEXT NOT NULL,
                value JSONB NOT NULL DEFAULT '{}',
                description TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS triggers (
                id TEXT PRIMARY KEY,
                target_path TEXT NOT NULL,
                target_is_flow BOOLEAN NOT NULL DEFAULT false,
                trigger_type TEXT NOT NULL DEFAULT 'cron',
                config JSONB NOT NULL DEFAULT '{}',
                enabled BOOLEAN NOT NULL DEFAULT true,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX IF NOT EXISTS idx_scripts_path ON scripts(path);
            CREATE INDEX IF NOT EXISTS idx_jobs_scheduled ON jobs(scheduled_for) WHERE NOT running;
            CREATE INDEX IF NOT EXISTS idx_runs_target ON runs(target_path);
            CREATE INDEX IF NOT EXISTS idx_runs_state ON runs(state);
            CREATE INDEX IF NOT EXISTS idx_graph_nodes_kind ON graph_nodes(kind);
            CREATE INDEX IF NOT EXISTS idx_graph_edges_source ON graph_edges(source);
            CREATE INDEX IF NOT EXISTS idx_triggers_enabled ON triggers(enabled);
            CREATE TABLE IF NOT EXISTS builds (
                hash TEXT PRIMARY KEY,
                artifact_path TEXT NOT NULL,
                built_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                build_mode TEXT NOT NULL DEFAULT 'debug',
                success BOOLEAN NOT NULL DEFAULT true
            );
            CREATE TABLE IF NOT EXISTS webhooks (
                id TEXT PRIMARY KEY,
                target_url TEXT NOT NULL,
                event TEXT NOT NULL,
                secret TEXT,
                enabled BOOLEAN NOT NULL DEFAULT true,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE IF NOT EXISTS executions (
                id TEXT PRIMARY KEY,
                flow_path TEXT,
                dag_label TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                steps JSONB NOT NULL DEFAULT '[]',
                started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                completed_at TIMESTAMPTZ,
                total_duration_ms BIGINT
            );
            CREATE INDEX IF NOT EXISTS idx_executions_status ON executions(status);
            CREATE INDEX IF NOT EXISTS idx_webhooks_event ON webhooks(event);
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Script operations ──

    pub async fn register_script(
        &self,
        path: &str,
        source: &str,
        version: &str,
        manifest: &serde_json::Value,
        deps: &[automaton_core::DepRef],
    ) -> Result<String, sqlx::Error> {
        use sha2::{Digest, Sha256};
        let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        sqlx::query(
            "INSERT INTO scripts (hash, path, version, source, manifest) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (hash) DO UPDATE SET path = EXCLUDED.path",
        )
        .bind(&hash)
        .bind(path)
        .bind(version)
        .bind(source)
        .bind(manifest)
        .execute(&self.pool)
        .await?;
        for dep in deps {
            sqlx::query(
                "INSERT INTO script_deps (script_hash, depends_on, version_req) VALUES ($1, $2, $3)
                 ON CONFLICT DO NOTHING",
            )
            .bind(&hash)
            .bind(&dep.name)
            .bind(&dep.version_req)
            .execute(&self.pool)
            .await?;
        }
        Ok(hash)
    }

    pub async fn get_script(&self, path: &str) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT hash, path, version, source, manifest, built, created_at FROM scripts WHERE path = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            serde_json::json!({
                "hash": r.get::<String, _>("hash"),
                "path": r.get::<String, _>("path"),
                "version": r.get::<String, _>("version"),
                "source": r.get::<String, _>("source"),
                "manifest": r.get::<serde_json::Value, _>("manifest"),
                "built": r.get::<bool, _>("built"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        }))
    }

    pub async fn list_scripts(&self) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT DISTINCT ON (path) hash, path, version, built, created_at FROM scripts ORDER BY path, created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| {
            serde_json::json!({
                "hash": r.get::<String, _>("hash"),
                "path": r.get::<String, _>("path"),
                "version": r.get::<String, _>("version"),
                "built": r.get::<bool, _>("built"),
            })
        }).collect())
    }

    pub async fn mark_built(&self, path: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE scripts SET built = true WHERE path = $1")
            .bind(path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Job queue operations ──

    pub async fn enqueue(&self, kind: &str, target: &str, args: &serde_json::Value) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO jobs (kind, target_path, args) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(kind)
        .bind(target)
        .bind(args)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("id"))
    }

    pub async fn dequeue(&self, worker_id: &str) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row = sqlx::query(
            "UPDATE jobs SET running = true, worker_id = $1
             WHERE id = (
                 SELECT id FROM jobs WHERE NOT running AND scheduled_for <= NOW()
                 ORDER BY priority DESC, created_at ASC LIMIT 1
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING id, kind, target_path, args, scheduled_for, priority",
        )
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            serde_json::json!({
                "id": r.get::<i64, _>("id"),
                "kind": r.get::<String, _>("kind"),
                "target_path": r.get::<String, _>("target_path"),
                "args": r.get::<serde_json::Value, _>("args"),
                "scheduled_for": r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_for"),
                "priority": r.get::<i32, _>("priority"),
            })
        }))
    }

    pub async fn complete_job(&self, job_id: i64, _run_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM jobs WHERE id = $1")
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_jobs(&self, limit: i64) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, kind, target_path, args, scheduled_for, priority, running, worker_id
             FROM jobs ORDER BY priority DESC, scheduled_for ASC LIMIT $1")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|r| {
            serde_json::json!({
                "id": r.get::<i64, _>("id"),
                "kind": r.get::<String, _>("kind"),
                "target_path": r.get::<String, _>("target_path"),
                "scheduled_for": r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_for"),
                "priority": r.get::<i32, _>("priority"),
                "running": r.get::<bool, _>("running"),
                "worker_id": r.get::<Option<String>, _>("worker_id"),
            })
        }).collect())
    }

    // ── Run operations ──

    pub async fn record_run(&self, id: &str, target: &str, kind: &str, args: &serde_json::Value) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO runs (id, target_path, kind, args) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(target)
        .bind(kind)
        .bind(args)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_run(&self, id: &str, state: &str, result: Option<&serde_json::Value>, error: Option<&str>, attempt: u32, duration_ms: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE runs SET state = $1, result = $2, error = $3, attempt = $4, duration_ms = $5, completed_at = NOW() WHERE id = $6",
        )
        .bind(state)
        .bind(result)
        .bind(error)
        .bind(attempt as i32)
        .bind(duration_ms)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_runs(&self, target: &str, limit: i64) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, target_path, kind, state, attempt, error, duration_ms, created_at, completed_at
             FROM runs WHERE target_path = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(target)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "target": r.get::<String, _>("target_path"),
                "kind": r.get::<String, _>("kind"),
                "state": r.get::<String, _>("state"),
                "attempt": r.get::<i32, _>("attempt"),
                "error": r.get::<Option<String>, _>("error"),
                "duration_ms": r.get::<i64, _>("duration_ms"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        }).collect())
    }

    // ── Graph operations ──

    pub async fn add_node(&self, kind: &str, name: &str, props: &serde_json::Value) -> Result<String, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO graph_nodes (id, kind, name, properties) VALUES ($1, $2, $3, $4)")
            .bind(&id)
            .bind(kind)
            .bind(name)
            .bind(props)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn add_edge(&self, source: &str, target: &str, kind: &str) -> Result<String, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO graph_edges (id, source, target, kind) VALUES ($1, $2, $3, $4)")
            .bind(&id)
            .bind(source)
            .bind(target)
            .bind(kind)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn get_nodes(&self, kind: Option<&str>) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = if let Some(k) = kind {
            sqlx::query("SELECT id, kind, name, properties, created_at FROM graph_nodes WHERE kind = $1 ORDER BY created_at")
                .bind(k)
                .fetch_all(&self.pool).await?
        } else {
            sqlx::query("SELECT id, kind, name, properties, created_at FROM graph_nodes ORDER BY created_at")
                .fetch_all(&self.pool).await?
        };
        Ok(rows.into_iter().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "kind": r.get::<String, _>("kind"),
                "name": r.get::<String, _>("name"),
                "properties": r.get::<serde_json::Value, _>("properties"),
            })
        }).collect())
    }

    pub async fn get_edges(&self) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query("SELECT id, source, target, kind, properties FROM graph_edges ORDER BY created_at")
            .fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "source": r.get::<String, _>("source"),
                "target": r.get::<String, _>("target"),
                "kind": r.get::<String, _>("kind"),
            })
        }).collect())
    }

    // ── Variable / Secret operations ──

    pub async fn set_variable(&self, path: &str, value: &str, is_secret: bool) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO variables (path, value, is_secret) VALUES ($1, $2, $3)
             ON CONFLICT (path) DO UPDATE SET value = EXCLUDED.value, is_secret = EXCLUDED.is_secret",
        )
        .bind(path)
        .bind(value)
        .bind(is_secret)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_variable(&self, path: &str) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query("SELECT value FROM variables WHERE path = $1")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("value")))
    }

    pub async fn list_variables(&self) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT path, is_secret, description, created_at FROM variables ORDER BY path",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| {
            serde_json::json!({
                "path": r.get::<String, _>("path"),
                "is_secret": r.get::<bool, _>("is_secret"),
                "description": r.get::<Option<String>, _>("description"),
            })
        }).collect())
    }

    // ── Resource operations ──

    pub async fn set_resource(&self, path: &str, rtype: &str, value: &serde_json::Value) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO resources (path, resource_type, value) VALUES ($1, $2, $3)
             ON CONFLICT (path) DO UPDATE SET resource_type = EXCLUDED.resource_type, value = EXCLUDED.value",
        )
        .bind(path)
        .bind(rtype)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_resource(&self, path: &str) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row = sqlx::query("SELECT resource_type, value, description FROM resources WHERE path = $1")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| serde_json::json!({
            "type": r.get::<String, _>("resource_type"),
            "value": r.get::<serde_json::Value, _>("value"),
        })))
    }

    pub async fn list_resources(&self, rtype: Option<&str>) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = if let Some(t) = rtype {
            sqlx::query("SELECT path, resource_type, description FROM resources WHERE resource_type = $1 ORDER BY path")
                .bind(t).fetch_all(&self.pool).await?
        } else {
            sqlx::query("SELECT path, resource_type, description FROM resources ORDER BY path")
                .fetch_all(&self.pool).await?
        };
        Ok(rows.into_iter().map(|r| serde_json::json!({
            "path": r.get::<String, _>("path"),
            "type": r.get::<String, _>("resource_type"),
        })).collect())
    }

    // ── Flow operations ──

    pub async fn store_flow(
        &self,
        path: &str,
        version: &str,
        definition: &serde_json::Value,
        _summary: Option<&str>,
        _on_failure: Option<&str>,
    ) -> Result<String, sqlx::Error> {
        use sha2::{Digest, Sha256};
        let hash = format!("{:x}", Sha256::digest(path.as_bytes()));
        sqlx::query(
            "INSERT INTO flows (hash, path, version, flow_definition) VALUES ($1, $2, $3, $4)
             ON CONFLICT (hash) DO UPDATE SET flow_definition = EXCLUDED.flow_definition, version = EXCLUDED.version"
        )
        .bind(&hash)
        .bind(path)
        .bind(version)
        .bind(definition)
        .execute(&self.pool)
        .await?;
        Ok(hash)
    }

    pub async fn get_flow(&self, path: &str) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT hash, path, version, flow_definition, created_at FROM flows WHERE path = $1"
        )
        .bind(path)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().next().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("hash"),
                "path": r.get::<String, _>("path"),
                "version": r.get::<String, _>("version"),
                "definition": r.get::<serde_json::Value, _>("flow_definition"),
            })
        }))
    }

    pub async fn list_flows(&self) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT hash, path, version, created_at FROM flows ORDER BY path"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("hash"),
                "path": r.get::<String, _>("path"),
                "version": r.get::<String, _>("version"),
            })
        }).collect())
    }

    pub async fn delete_flow(&self, path: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM flows WHERE path = $1")
            .bind(path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Trigger operations ──

    pub async fn create_trigger(&self, target: &str, is_flow: bool, ttype: &str, config: &serde_json::Value) -> Result<String, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO triggers (id, target_path, target_is_flow, trigger_type, config) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id)
        .bind(target)
        .bind(is_flow)
        .bind(ttype)
        .bind(config)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn get_enabled_triggers(&self, ttype: &str) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, target_path, target_is_flow, config, created_at FROM triggers WHERE enabled AND trigger_type = $1",
        )
        .bind(ttype)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| serde_json::json!({
            "id": r.get::<String, _>("id"),
            "target_path": r.get::<String, _>("target_path"),
            "target_is_flow": r.get::<bool, _>("target_is_flow"),
            "config": r.get::<serde_json::Value, _>("config"),
        })).collect())
    }

    // ── Build recording ──

    pub async fn record_build(&self, hash: &str, artifact_path: &str, mode: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO builds (hash, artifact_path, build_mode, success) VALUES ($1, $2, $3, true)
             ON CONFLICT (hash) DO UPDATE SET artifact_path = $2, build_mode = $3"
        )
        .bind(hash)
        .bind(artifact_path)
        .bind(mode)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Webhook management ──

    pub async fn register_webhook(&self, target_url: &str, event: &str, secret: Option<&str>) -> Result<String, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO webhooks (id, target_url, event, secret) VALUES ($1, $2, $3, $4)",
        )
        .bind(&id)
        .bind(target_url)
        .bind(event)
        .bind(secret)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn list_webhooks(&self, event: Option<&str>) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = if let Some(ev) = event {
            sqlx::query(
                "SELECT id, target_url, event, secret, enabled, created_at FROM webhooks WHERE event = $1 ORDER BY created_at",
            )
            .bind(ev)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, target_url, event, secret, enabled, created_at FROM webhooks ORDER BY created_at",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "target_url": r.get::<String, _>("target_url"),
                "event": r.get::<String, _>("event"),
                "secret": r.get::<Option<String>, _>("secret"),
                "enabled": r.get::<bool, _>("enabled"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        }).collect())
    }

    pub async fn delete_webhook(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM webhooks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Execution history ──

    pub async fn store_execution(&self, execution: &FlowExecution) -> Result<(), sqlx::Error> {
        let steps_json = serde_json::to_value(&execution.steps).unwrap_or_default();
        let status_str = serde_json::to_string(&execution.status).unwrap_or_default();
        sqlx::query(
            "INSERT INTO executions (id, flow_path, dag_label, status, steps, started_at, completed_at, total_duration_ms) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(execution.execution_id.to_string())
        .bind(&execution.flow_path)
        .bind(&execution.dag_label)
        .bind(&status_str)
        .bind(steps_json)
        .bind(execution.started_at)
        .bind(execution.completed_at)
        .bind(execution.total_duration_ms.map(|d| d as i64))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_executions(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, flow_path, dag_label, status, steps, started_at, completed_at, total_duration_ms FROM executions ORDER BY started_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "flow_path": r.get::<Option<String>, _>("flow_path"),
                "dag_label": r.get::<Option<String>, _>("dag_label"),
                "status": r.get::<String, _>("status"),
                "steps": r.get::<serde_json::Value, _>("steps"),
                "started_at": r.get::<chrono::DateTime<chrono::Utc>, _>("started_at"),
                "completed_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("completed_at"),
                "total_duration_ms": r.get::<Option<i64>, _>("total_duration_ms"),
            })
        }).collect())
    }

    pub async fn get_trigger_by_id(&self, id: &str) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, target_path, target_is_flow, trigger_type, config, enabled, created_at FROM triggers WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().next().map(|r| serde_json::json!({
            "id": r.get::<String, _>("id"),
            "target_path": r.get::<String, _>("target_path"),
            "target_is_flow": r.get::<bool, _>("target_is_flow"),
            "trigger_type": r.get::<String, _>("trigger_type"),
            "config": r.get::<serde_json::Value, _>("config"),
            "enabled": r.get::<bool, _>("enabled"),
        })))
    }
}

#[async_trait::async_trait]
impl automaton_core::backend::RegistryBackend for AutomatonDb {
    async fn register_module(&self, path: &str, source: &str, manifest: &automaton_core::AutomationManifest) -> automaton_core::Result<automaton_core::ModuleId> {
        let manifest_val = serde_json::to_value(manifest).unwrap_or_default();
        let deps: Vec<automaton_core::DepRef> = manifest.depends_on.clone();
        self.register_script(path, source, &manifest.version, &manifest_val, &deps).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        let hash = automaton_core::ContentHash::compute(source.as_bytes());
        let version = semver::Version::parse(&manifest.version)
            .map_err(|e| automaton_core::AutomatonError::Other(e.to_string()))?;
        Ok(automaton_core::ModuleId {
            path: path.to_string(),
            version,
            hash,
            created_at: chrono::Utc::now(),
        })
    }

    async fn get_module(&self, path: &str) -> automaton_core::Result<Option<automaton_core::AutomationModule>> {
        let script = self.get_script(path).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        match script {
            Some(v) => {
                let source = v.get("source").and_then(|s| s.as_str()).unwrap_or("").to_string();
                let hash_str = v.get("hash").and_then(|h| h.as_str()).unwrap_or("");
                let manifest_val = v.get("manifest").cloned().unwrap_or_default();
                let manifest: automaton_core::AutomationManifest = serde_json::from_value(manifest_val)
                    .unwrap_or_else(|_| automaton_core::AutomationManifest {
                        name: path.to_string(),
                        version: "0.1.0".to_string(),
                        entry: "main".to_string(),
                        summary: None,
                        description: None,
                        timeout_ms: 30_000,
                        retry: None,
                        permissions: vec![],
                        depends_on: vec![],
                        resources: vec![],
                        tags: vec![],
                        require_approval: false,
                        inputs_schema: automaton_core::SchemaMode::Auto,
                        outputs_schema: automaton_core::SchemaMode::Auto,
                    });
                let built = v.get("built").and_then(|b| b.as_bool()).unwrap_or(false);
                Ok(Some(automaton_core::AutomationModule {
                    source,
                    hash: automaton_core::ContentHash(hash_str.to_string()),
                    manifest,
                    built,
                }))
            }
            None => Ok(None),
        }
    }

    async fn record_run(&self, run_id: &str, module_path: &str, input: &serde_json::Value) -> automaton_core::Result<()> {
        self.record_run(run_id, module_path, "backend", input).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn update_run(
        &self,
        run_id: &str,
        status: &str,
        output: Option<&serde_json::Value>,
        error_msg: Option<&str>,
        attempt: u32,
    ) -> automaton_core::Result<()> {
        self.update_run(run_id, status, output, error_msg, attempt, 0).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Build cache directory ──

    fn build_cache_dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::from("./data/builds")
    }

    async fn resolve_references(&self, val: &serde_json::Value) -> automaton_core::Result<serde_json::Value> {
        Ok(val.clone())
    }

    async fn enqueue_job(&self, kind: &str, target: &str, args: &serde_json::Value) -> automaton_core::Result<i64> {
        self.enqueue(kind, target, args).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn dequeue_job(&self, worker_id: &str) -> automaton_core::Result<Option<serde_json::Value>> {
        self.dequeue(worker_id).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn complete_job(&self, job_id: i64) -> automaton_core::Result<()> {
        self.complete_job(job_id, "").await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_jobs(&self, limit: i64) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.list_jobs(limit).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn create_trigger(&self, target: &str, is_flow: bool, ttype: &str, config: &serde_json::Value) -> automaton_core::Result<String> {
        self.create_trigger(target, is_flow, ttype, config).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn get_enabled_triggers(&self, ttype: &str) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.get_enabled_triggers(ttype).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn get_variable(&self, path: &str) -> automaton_core::Result<Option<String>> {
        self.get_variable(path).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn set_variable(&self, path: &str, value: &str, is_secret: bool) -> automaton_core::Result<()> {
        self.set_variable(path, value, is_secret).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_variables(&self) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.list_variables().await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn get_resource(&self, path: &str) -> automaton_core::Result<Option<serde_json::Value>> {
        self.get_resource(path).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn set_resource(&self, path: &str, resource_type: &str, value: &serde_json::Value) -> automaton_core::Result<()> {
        self.set_resource(path, resource_type, value).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_resources(&self, resource_type: Option<&str>) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.list_resources(resource_type).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn store_flow(&self, path: &str, version: &str, definition: &serde_json::Value, summary: Option<&str>, on_failure: Option<&str>) -> automaton_core::Result<String> {
        self.store_flow(path, version, definition, summary, on_failure).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn get_flow(&self, path: &str) -> automaton_core::Result<Option<serde_json::Value>> {
        self.get_flow(path).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn list_flows(&self) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.list_flows().await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn delete_flow(&self, path: &str) -> automaton_core::Result<()> {
        self.delete_flow(path).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Extra trait methods that delegate to existing self methods ──

    async fn list_modules(&self) -> automaton_core::Result<Vec<(String, String, String, bool)>> {
        let scripts = self.list_scripts().await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(scripts.into_iter().map(|v| {
            let path = v.get("path").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let version = v.get("version").and_then(|s| s.as_str()).unwrap_or("0.1.0").to_string();
            let hash = v.get("hash").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let built = v.get("built").and_then(|b| b.as_bool()).unwrap_or(false);
            (path, version, hash, built)
        }).collect())
    }

    async fn mark_built(&self, path: &str) -> automaton_core::Result<()> {
        self.mark_built(path).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn record_build(&self, hash: &str, artifact_path: &str, mode: &str) -> automaton_core::Result<()> {
        self.record_build(hash, artifact_path, mode).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get_runs(&self, module_path: &str) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.get_runs(module_path, 50).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn register_webhook(&self, target_url: &str, event: &str, secret: Option<&str>) -> automaton_core::Result<String> {
        self.register_webhook(target_url, event, secret).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn list_webhooks(&self, event: Option<&str>) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.list_webhooks(event).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }

    async fn delete_webhook(&self, id: &str) -> automaton_core::Result<()> {
        self.delete_webhook(id).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn store_execution(&self, execution: &FlowExecution) -> automaton_core::Result<()> {
        self.store_execution(execution).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_executions(&self, limit: i64, offset: i64) -> automaton_core::Result<Vec<serde_json::Value>> {
        self.list_executions(limit, offset).await
            .map_err(|e| automaton_core::AutomatonError::Database(e.to_string()))
    }
}
