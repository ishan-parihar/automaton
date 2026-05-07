use std::path::{Path, PathBuf};
use std::sync::Mutex;

use automaton_core::*;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

/// Local module registry backed by SQLite.
pub struct Registry {
    db: Mutex<Connection>,
    #[allow(dead_code)]
    data_dir: PathBuf,
    build_cache: PathBuf,
    secret_keeper: Option<SecretKeeper>,
}

fn with_registry<T>(
    db: &Mutex<Connection>,
    f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> Result<T> {
    let conn = db
        .lock()
        .map_err(|e| AutomatonError::Database(e.to_string()))?;
    f(&conn).map_err(AutomatonError::from)
}

impl Registry {
    pub fn open(data_dir: &Path) -> Result<Self> {
        let db_path = data_dir.join("registry.db");
        let build_cache = data_dir.join("builds");
        std::fs::create_dir_all(&build_cache)?;
        let db = Connection::open(&db_path)?;
        let secret_keeper = std::env::var("AUTOMATON_MASTER_KEY")
            .ok()
            .filter(|k| k.len() == 64)
            .map(|_| SecretKeeper::from_env());
        let registry = Self {
            db: Mutex::new(db),
            data_dir: data_dir.to_path_buf(),
            build_cache,
            secret_keeper,
        };
        registry.init_tables()?;
        Ok(registry)
    }

    fn init_tables(&self) -> Result<()> {
        with_registry(&self.db, |db| {
            db.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS modules (
                    path TEXT PRIMARY KEY,
                    version TEXT NOT NULL,
                    hash TEXT NOT NULL UNIQUE,
                    source TEXT NOT NULL,
                    manifest TEXT NOT NULL,
                    built INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS dependencies (
                    module_path TEXT NOT NULL,
                    depends_on TEXT NOT NULL,
                    version_req TEXT,
                    PRIMARY KEY (module_path, depends_on),
                    FOREIGN KEY (module_path) REFERENCES modules(path)
                );
                CREATE TABLE IF NOT EXISTS builds (
                    hash TEXT PRIMARY KEY,
                    artifact_path TEXT NOT NULL,
                    built_at TEXT NOT NULL DEFAULT (datetime('now')),
                    build_mode TEXT NOT NULL DEFAULT 'debug',
                    success INTEGER NOT NULL DEFAULT 1
                );
                CREATE TABLE IF NOT EXISTS runs (
                    id TEXT PRIMARY KEY,
                    module_path TEXT NOT NULL,
                    input TEXT,
                    output TEXT,
                    state TEXT NOT NULL DEFAULT 'pending',
                    attempt INTEGER NOT NULL DEFAULT 1,
                    error TEXT,
                    started_at TEXT,
                    completed_at TEXT,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS resources (
                    path TEXT PRIMARY KEY,
                    resource_type TEXT NOT NULL,
                    value TEXT NOT NULL DEFAULT '{}',
                    description TEXT,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS variables (
                    path TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    is_secret INTEGER NOT NULL DEFAULT 1,
                    description TEXT,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS triggers (
                    id TEXT PRIMARY KEY,
                    target_path TEXT NOT NULL,
                    target_is_flow INTEGER NOT NULL DEFAULT 0,
                    trigger_type TEXT NOT NULL DEFAULT 'cron',
                    config TEXT NOT NULL DEFAULT '{}',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS graph_nodes (
                    id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    name TEXT NOT NULL,
                    properties TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS graph_edges (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL REFERENCES graph_nodes(id),
                    target TEXT NOT NULL REFERENCES graph_nodes(id),
                    kind TEXT NOT NULL,
                    properties TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_graphedges_source ON graph_edges(source);
                CREATE INDEX IF NOT EXISTS idx_graphedges_target ON graph_edges(target);
                CREATE TABLE IF NOT EXISTS flows (
                    id TEXT PRIMARY KEY,
                    path TEXT NOT NULL,
                    version TEXT NOT NULL DEFAULT '0.1.0',
                    definition TEXT NOT NULL DEFAULT '{}',
                    summary TEXT,
                    on_failure TEXT,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS jobs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    kind TEXT NOT NULL DEFAULT 'script',
                    target_path TEXT NOT NULL,
                    args TEXT NOT NULL DEFAULT '{}',
                    scheduled_for TEXT NOT NULL DEFAULT (datetime('now')),
                    priority INTEGER NOT NULL DEFAULT 0,
                    running INTEGER NOT NULL DEFAULT 0,
                    worker_id TEXT,
                    max_attempts INTEGER NOT NULL DEFAULT 3,
                    attempt INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS webhooks (
                    id TEXT PRIMARY KEY,
                    target_url TEXT NOT NULL,
                    event TEXT NOT NULL,
                    secret TEXT,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS executions (
                    id TEXT PRIMARY KEY,
                    flow_path TEXT,
                    dag_label TEXT,
                    status TEXT NOT NULL DEFAULT 'pending',
                    steps JSONB NOT NULL DEFAULT '[]',
                    started_at TEXT NOT NULL DEFAULT (datetime('now')),
                    completed_at TEXT,
                    total_duration_ms BIGINT
                );
                ",
            )?;
            Ok(())
        })
    }

    pub fn register(
        &self,
        path: &str,
        source: &str,
        manifest: &AutomationManifest,
    ) -> Result<ModuleId> {
        let hash = ContentHash::compute(source.as_bytes());
        let now = chrono::Utc::now();
        let manifest_json = serde_json::to_string(manifest)?;

        with_registry(&self.db, |db| {
            db.execute(
                "INSERT OR REPLACE INTO modules (path, version, hash, source, manifest, built, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
                rusqlite::params![path, manifest.version, hash.as_str(), source, manifest_json, now.to_rfc3339()],
            )?;

            for dep in &manifest.depends_on {
                db.execute(
                    "INSERT OR REPLACE INTO dependencies (module_path, depends_on, version_req) VALUES (?1, ?2, ?3)",
                    rusqlite::params![path, dep.name, dep.version_req],
                )?;
            }
            Ok(())
        })?;

        Ok(ModuleId {
            path: path.to_string(),
            version: semver::Version::parse(&manifest.version)
                .map_err(|e: semver::Error| AutomatonError::Other(e.to_string()))?,
            hash,
            created_at: now,
        })
    }

    pub fn get(&self, path: &str) -> Result<Option<AutomationModule>> {
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT path, version, hash, source, manifest, built FROM modules WHERE path = ?1",
            )?;
            let mut rows = stmt.query(rusqlite::params![path])?;
            match rows.next()? {
                Some(row) => {
                    let manifest_json: String = row.get(4)?;
                    let manifest: AutomationManifest = serde_json::from_str(&manifest_json)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    Ok(Some(AutomationModule {
                        manifest,
                        source: row.get(3)?,
                        hash: ContentHash(row.get(2)?),
                        built: row.get(5)?,
                    }))
                }
                None => Ok(None),
            }
        })
    }

    pub fn list(&self) -> Result<Vec<(String, String, String, bool)>> {
        with_registry(&self.db, |db| {
            let mut stmt =
                db.prepare("SELECT path, version, hash, built FROM modules ORDER BY path")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    pub fn mark_built(&self, path: &str) -> Result<()> {
        with_registry(&self.db, |db| {
            db.execute(
                "UPDATE modules SET built = 1 WHERE path = ?1",
                rusqlite::params![path],
            )?;
            Ok(())
        })
    }

    pub fn record_build(&self, hash: &str, artifact_path: &str, mode: &str) -> Result<()> {
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT OR REPLACE INTO builds (hash, artifact_path, built_at, build_mode, success) VALUES (?1, ?2, datetime('now'), ?3, 1)",
                rusqlite::params![hash, artifact_path, mode],
            )?;
            Ok(())
        })
    }

    pub fn record_run(
        &self,
        run_id: &str,
        module_path: &str,
        input: &serde_json::Value,
    ) -> Result<()> {
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO runs (id, module_path, input, state) VALUES (?1, ?2, ?3, 'pending')",
                rusqlite::params![
                    run_id,
                    module_path,
                    serde_json::to_string(input).unwrap_or_default()
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_run(
        &self,
        run_id: &str,
        state: &str,
        output: Option<&serde_json::Value>,
        error: Option<&str>,
        attempt: u32,
    ) -> Result<()> {
        with_registry(&self.db, |db| {
            db.execute(
                "UPDATE runs SET state = ?1, output = ?2, error = ?3, attempt = ?4, completed_at = datetime('now') WHERE id = ?5",
                rusqlite::params![
                    state,
                    output.map(|v| serde_json::to_string(v).unwrap_or_default()),
                    error,
                    attempt,
                    run_id,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_runs(&self, module_path: &str) -> Result<Vec<serde_json::Value>> {
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, state, attempt, error, created_at, completed_at FROM runs WHERE module_path = ?1 ORDER BY created_at DESC LIMIT 50",
            )?;
            let rows = stmt.query_map(rusqlite::params![module_path], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "state": row.get::<_, String>(1)?,
                    "attempt": row.get::<_, u32>(2)?,
                    "error": row.get::<_, Option<String>>(3)?,
                    "created_at": row.get::<_, String>(4)?,
                    "completed_at": row.get::<_, Option<String>>(5)?,
                }))
            })?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    pub fn build_cache_dir(&self) -> &Path {
        &self.build_cache
    }

    pub fn compute_hash(source: &str, manifest: &AutomationManifest) -> ContentHash {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.update(manifest.name.as_bytes());
        hasher.update(manifest.version.as_bytes());
        ContentHash(format!("{:x}", hasher.finalize()))
    }

    // ── Variable / Secret operations ──

    pub fn set_variable(&self, path: &str, value: &str, is_secret: bool) -> Result<()> {
        let p = path.to_string();
        // Encrypt if secret and we have a key
        let v = if is_secret {
            if let Some(sk) = &self.secret_keeper {
                sk.encrypt(value)
            } else {
                value.to_string()
            }
        } else {
            value.to_string()
        };
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO variables (path, value, is_secret) VALUES (?1, ?2, ?3) ON CONFLICT(path) DO UPDATE SET value=excluded.value, is_secret=excluded.is_secret",
                rusqlite::params![p, v, is_secret as i32],
            )?;
            Ok(())
        })
    }

    pub fn get_variable(&self, path: &str) -> Result<Option<String>> {
        let p = path.to_string();
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare("SELECT value, is_secret FROM variables WHERE path = ?1")?;
            let mut rows = stmt.query(rusqlite::params![p])?;
            match rows.next()? {
                Some(row) => {
                    let stored: String = row.get(0)?;
                    let is_secret: bool = row.get(1)?;
                    // Decrypt if secret
                    let val = if is_secret {
                        if let Some(sk) = &self.secret_keeper {
                            sk.decrypt(&stored).unwrap_or(stored)
                        } else {
                            stored
                        }
                    } else {
                        stored
                    };
                    Ok(Some(val))
                }
                None => Ok(None),
            }
        })
    }

    pub fn list_variables(&self) -> Result<Vec<serde_json::Value>> {
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT path, value, is_secret, created_at FROM variables ORDER BY path",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(serde_json::json!({
                    "path": row.get::<_, String>(0)?,
                    "value": row.get::<_, String>(1)?,
                    "is_secret": row.get::<_, bool>(2)?,
                    "created_at": row.get::<_, String>(3)?,
                }))
            })?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    // ── Resource operations ──

    pub fn set_resource(
        &self,
        path: &str,
        resource_type: &str,
        value: &serde_json::Value,
    ) -> Result<()> {
        let p = path.to_string();
        let rt = resource_type.to_string();
        let v = serde_json::to_string(value).map_err(|e| AutomatonError::Other(e.to_string()))?;
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO resources (path, resource_type, value) VALUES (?1, ?2, ?3) ON CONFLICT(path) DO UPDATE SET resource_type=excluded.resource_type, value=excluded.value",
                rusqlite::params![p, rt, v],
            )?;
            Ok(())
        })
    }

    pub fn get_resource(&self, path: &str) -> Result<Option<serde_json::Value>> {
        let p = path.to_string();
        with_registry(&self.db, |db| {
            let mut stmt =
                db.prepare("SELECT resource_type, value FROM resources WHERE path = ?1")?;
            let mut rows = stmt.query(rusqlite::params![p])?;
            match rows.next()? {
                Some(row) => {
                    let val_str: String = row.get(1)?;
                    Ok(Some(serde_json::json!({
                        "resource_type": row.get::<_, String>(0)?,
                        "value": serde_json::from_str::<serde_json::Value>(&val_str).unwrap_or_default(),
                    })))
                }
                None => Ok(None),
            }
        })
    }

    pub fn list_resources(&self, resource_type: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let rt = resource_type.map(|s| s.to_string());
        with_registry(&self.db, |db| {
            let rows = if let Some(ref t) = rt {
                let mut stmt = db.prepare("SELECT path, resource_type FROM resources WHERE resource_type = ?1 ORDER BY path")?;
                let rows = stmt.query_map(rusqlite::params![t], |row| {
                    Ok(serde_json::json!({"path": row.get::<_, String>(0)?, "type": row.get::<_, String>(1)?}))
                })?;
                let mut r = vec![];
                for row in rows {
                    r.push(row?);
                }
                r
            } else {
                let mut stmt =
                    db.prepare("SELECT path, resource_type FROM resources ORDER BY path")?;
                let rows = stmt.query_map([], |row| {
                    Ok(serde_json::json!({"path": row.get::<_, String>(0)?, "type": row.get::<_, String>(1)?}))
                })?;
                let mut r = vec![];
                for row in rows {
                    r.push(row?);
                }
                r
            };
            Ok(rows)
        })
    }

    /// Resolve `$var:path` and `$res:path` references in a JSON value.
    /// Recursively walks the value replacing `$var:X` with variable X's value
    /// and `$res:X` with resource X's JSON value.
    pub fn resolve_references(&self, val: &serde_json::Value) -> Result<serde_json::Value> {
        match val {
            serde_json::Value::String(s) => {
                if let Some(var_path) = s.strip_prefix("$var:") {
                    match self.get_variable(var_path)? {
                        Some(v) => Ok(serde_json::Value::String(v)),
                        None => Ok(val.clone()),
                    }
                } else if let Some(res_path) = s.strip_prefix("$res:") {
                    match self.get_resource(res_path)? {
                        Some(v) => Ok(v),
                        None => Ok(val.clone()),
                    }
                } else {
                    Ok(val.clone())
                }
            }
            serde_json::Value::Object(map) => {
                let mut resolved = serde_json::Map::new();
                for (k, v) in map {
                    resolved.insert(k.clone(), self.resolve_references(v)?);
                }
                Ok(serde_json::Value::Object(resolved))
            }
            serde_json::Value::Array(arr) => {
                let resolved: Result<Vec<_>> = arr.iter().map(|v| self.resolve_references(v)).collect();
                Ok(serde_json::Value::Array(resolved?))
            }
            other => Ok(other.clone()),
        }
    }

    // ── Job queue operations ──

    pub fn enqueue(&self, kind: &str, target: &str, args: &serde_json::Value) -> Result<i64> {
        let k = kind.to_string();
        let t = target.to_string();
        let a = serde_json::to_string(args).map_err(|e| AutomatonError::Other(e.to_string()))?;
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO jobs (kind, target_path, args) VALUES (?1, ?2, ?3)",
                rusqlite::params![k, t, a],
            )?;
            Ok(db.last_insert_rowid())
        })
    }

    pub fn list_jobs(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, kind, target_path, args, scheduled_for, priority, running, worker_id, attempt FROM jobs ORDER BY priority DESC, scheduled_for ASC LIMIT ?1",
            )?;
            let rows = stmt.query_map(rusqlite::params![limit], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "target_path": row.get::<_, String>(2)?,
                    "scheduled_for": row.get::<_, String>(4)?,
                    "priority": row.get::<_, i32>(5)?,
                    "running": row.get::<_, bool>(6)?,
                    "attempt": row.get::<_, i32>(8)?,
                }))
            })?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    /// Dequeue the highest-priority pending job (sets running=true, assigns worker_id)
    pub fn dequeue(&self, worker_id: &str) -> Result<Option<serde_json::Value>> {
        let wid = worker_id.to_string();
        with_registry(&self.db, |db| {
            // SQLite: simple LIMIT 1 approach (no SKIP LOCKED in SQLite)
            let mut stmt = db.prepare(
                "SELECT id, kind, target_path, args FROM jobs WHERE NOT running AND scheduled_for <= datetime('now') ORDER BY priority DESC, scheduled_for ASC LIMIT 1"
            )?;
            let mut rows = stmt.query([])?;
            if let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let kind: String = row.get(1)?;
                let target: String = row.get(2)?;
                let args_str: String = row.get(3)?;
                // Mark as running (with race condition guard)
                db.execute(
                    "UPDATE jobs SET running = 1, worker_id = ?1, attempt = attempt + 1 WHERE id = ?2 AND NOT running",
                    rusqlite::params![wid, id],
                )?;
                if db.changes() > 0 {
                    Ok(Some(serde_json::json!({
                        "id": id,
                        "kind": kind,
                        "target_path": target,
                        "args": serde_json::from_str::<serde_json::Value>(&args_str).unwrap_or_default(),
                    })))
                } else {
                    Ok(None) // Race condition: another worker claimed it
                }
            } else {
                Ok(None)
            }
        })
    }

    /// Mark a job as completed and remove it from the queue
    pub fn complete_job(&self, job_id: i64) -> Result<()> {
        with_registry(&self.db, |db| {
            db.execute("DELETE FROM jobs WHERE id = ?1", rusqlite::params![job_id])?;
            Ok(())
        })
    }

    // ── Flow operations ──

    pub fn store_flow(
        &self,
        path: &str,
        version: &str,
        definition: &serde_json::Value,
        summary: Option<&str>,
        on_failure: Option<&str>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let i = id.clone();
        let p = path.to_string();
        let v = version.to_string();
        let d = serde_json::to_string(definition).map_err(|e| AutomatonError::Other(e.to_string()))?;
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO flows (id, path, version, definition, summary, on_failure) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET definition=excluded.definition, version=excluded.version",
                rusqlite::params![i, p, v, d, summary, on_failure],
            )?;
            Ok(id)
        })
    }

    pub fn get_flow(&self, path: &str) -> Result<Option<serde_json::Value>> {
        let p = path.to_string();
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, path, version, definition, summary, on_failure, created_at FROM flows WHERE path = ?1",
            )?;
            let mut rows = stmt.query(rusqlite::params![p])?;
            match rows.next()? {
                Some(row) => {
                    let def_str: String = row.get(3)?;
                    let definition: serde_json::Value =
                        serde_json::from_str(&def_str).unwrap_or_default();
                    Ok(Some(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "path": row.get::<_, String>(1)?,
                        "version": row.get::<_, String>(2)?,
                        "definition": definition,
                        "summary": row.get::<_, Option<String>>(4)?,
                        "on_failure": row.get::<_, Option<String>>(5)?,
                        "created_at": row.get::<_, String>(6)?,
                    })))
                }
                None => Ok(None),
            }
        })
    }

    pub fn list_flows(&self) -> Result<Vec<serde_json::Value>> {
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, path, version, created_at FROM flows ORDER BY path",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "path": row.get::<_, String>(1)?,
                    "version": row.get::<_, String>(2)?,
                    "created_at": row.get::<_, String>(3)?,
                }))
            })?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    pub fn delete_flow(&self, path: &str) -> Result<()> {
        let p = path.to_string();
        with_registry(&self.db, |db| {
            db.execute("DELETE FROM flows WHERE path = ?1", rusqlite::params![p])?;
            Ok(())
        })
    }

    // ── Trigger operations ──

    pub fn create_trigger(
        &self,
        target: &str,
        is_flow: bool,
        ttype: &str,
        config: &serde_json::Value,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let i = id.clone();
        let t = target.to_string();
        let tp = ttype.to_string();
        let c = serde_json::to_string(config).map_err(|e| AutomatonError::Other(e.to_string()))?;
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO triggers (id, target_path, target_is_flow, trigger_type, config) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![i, t, is_flow as i32, tp, c],
            )?;
            Ok(id)
        })
    }

    pub fn get_enabled_triggers(&self, ttype: &str) -> Result<Vec<serde_json::Value>> {
        let tp = ttype.to_string();
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, target_path, target_is_flow, config, created_at FROM triggers WHERE enabled AND trigger_type = ?1",
            )?;
            let rows = stmt.query_map(rusqlite::params![tp], |row| {
                let config_str: String = row.get(3)?;
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "target_path": row.get::<_, String>(1)?,
                    "target_is_flow": row.get::<_, bool>(2)?,
                    "config": serde_json::from_str::<serde_json::Value>(&config_str).unwrap_or_default(),
                    "created_at": row.get::<_, String>(4)?,
                }))
            })?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    // ── Webhook management ──

    pub fn register_webhook(&self, target_url: &str, event: &str, secret: Option<&str>) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let i = id.clone();
        let u = target_url.to_string();
        let e = event.to_string();
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO webhooks (id, target_url, event, secret) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![i, u, e, secret],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn list_webhooks(&self, event: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let e = event.map(|s| s.to_string());
        with_registry(&self.db, |db| {
            let rows = if let Some(ref ev) = e {
                let mut stmt = db.prepare(
                    "SELECT id, target_url, event, secret, enabled, created_at FROM webhooks WHERE event = ?1 ORDER BY created_at"
                )?;
                let rows = stmt.query_map(rusqlite::params![ev], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "target_url": row.get::<_, String>(1)?,
                        "event": row.get::<_, String>(2)?,
                        "secret": row.get::<_, Option<String>>(3)?,
                        "enabled": row.get::<_, bool>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                    }))
                })?;
                let mut r = vec![];
                for row in rows {
                    r.push(row?);
                }
                r
            } else {
                let mut stmt = db.prepare(
                    "SELECT id, target_url, event, secret, enabled, created_at FROM webhooks ORDER BY created_at"
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "target_url": row.get::<_, String>(1)?,
                        "event": row.get::<_, String>(2)?,
                        "secret": row.get::<_, Option<String>>(3)?,
                        "enabled": row.get::<_, bool>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                    }))
                })?;
                let mut r = vec![];
                for row in rows {
                    r.push(row?);
                }
                r
            };
            Ok(rows)
        })
    }

    pub fn delete_webhook(&self, id: &str) -> Result<()> {
        let i = id.to_string();
        with_registry(&self.db, |db| {
            db.execute("DELETE FROM webhooks WHERE id = ?1", rusqlite::params![i])?;
            Ok(())
        })
    }

    // ── Execution history ──

    pub fn store_execution(&self, execution: &FlowExecution) -> Result<()> {
        let steps_json = serde_json::to_string(&execution.steps).map_err(|e| AutomatonError::Other(e.to_string()))?;
        with_registry(&self.db, |db| {
            db.execute(
                "INSERT INTO executions (id, flow_path, dag_label, status, steps, started_at, completed_at, total_duration_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    execution.execution_id.to_string(),
                    execution.flow_path,
                    execution.dag_label,
                    serde_json::to_string(&execution.status).unwrap_or_default(),
                    steps_json,
                    execution.started_at.to_rfc3339(),
                    execution.completed_at.map(|t| t.to_rfc3339()),
                    execution.total_duration_ms.map(|d| d as i64),
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_executions(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>> {
        with_registry(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, flow_path, dag_label, status, steps, started_at, completed_at, total_duration_ms FROM executions ORDER BY started_at DESC LIMIT ?1 OFFSET ?2"
            )?;
            let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
                let steps_str: String = row.get(4)?;
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "flow_path": row.get::<_, Option<String>>(1)?,
                    "dag_label": row.get::<_, Option<String>>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "steps": serde_json::from_str::<serde_json::Value>(&steps_str).unwrap_or_default(),
                    "started_at": row.get::<_, String>(5)?,
                    "completed_at": row.get::<_, Option<String>>(6)?,
                    "total_duration_ms": row.get::<_, Option<i64>>(7)?,
                }))
            })?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }
}

#[async_trait::async_trait]
impl automaton_core::backend::RegistryBackend for Registry {
    async fn register_module(&self, path: &str, source: &str, manifest: &AutomationManifest) -> Result<ModuleId> {
        Registry::register(self, path, source, manifest)
    }

    async fn get_module(&self, path: &str) -> Result<Option<AutomationModule>> {
        Registry::get(self, path)
    }

    async fn list_modules(&self) -> Result<Vec<(String, String, String, bool)>> {
        Registry::list(self)
    }

    async fn mark_built(&self, path: &str) -> Result<()> {
        Registry::mark_built(self, path)
    }

    async fn record_build(&self, hash: &str, artifact_path: &str, mode: &str) -> Result<()> {
        Registry::record_build(self, hash, artifact_path, mode)
    }

    async fn record_run(&self, run_id: &str, module_path: &str, input: &serde_json::Value) -> Result<()> {
        Registry::record_run(self, run_id, module_path, input)
    }

    async fn update_run(
        &self,
        run_id: &str,
        status: &str,
        output: Option<&serde_json::Value>,
        error_msg: Option<&str>,
        attempt: u32,
    ) -> Result<()> {
        Registry::update_run(self, run_id, status, output, error_msg, attempt)
    }

    async fn get_runs(&self, module_path: &str) -> Result<Vec<serde_json::Value>> {
        Registry::get_runs(self, module_path)
    }

    fn build_cache_dir(&self) -> std::path::PathBuf {
        Registry::build_cache_dir(self).to_path_buf()
    }

    async fn resolve_references(&self, val: &serde_json::Value) -> Result<serde_json::Value> {
        Registry::resolve_references(self, val)
    }

    async fn enqueue_job(&self, kind: &str, target: &str, args: &serde_json::Value) -> Result<i64> {
        Registry::enqueue(self, kind, target, args)
    }

    async fn dequeue_job(&self, worker_id: &str) -> Result<Option<serde_json::Value>> {
        Registry::dequeue(self, worker_id)
    }

    async fn complete_job(&self, job_id: i64) -> Result<()> {
        Registry::complete_job(self, job_id)
    }

    async fn list_jobs(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        Registry::list_jobs(self, limit)
    }

    async fn create_trigger(&self, target: &str, is_flow: bool, ttype: &str, config: &serde_json::Value) -> Result<String> {
        Registry::create_trigger(self, target, is_flow, ttype, config)
    }

    async fn get_enabled_triggers(&self, ttype: &str) -> Result<Vec<serde_json::Value>> {
        Registry::get_enabled_triggers(self, ttype)
    }

    async fn get_variable(&self, path: &str) -> Result<Option<String>> {
        Registry::get_variable(self, path)
    }

    async fn set_variable(&self, path: &str, value: &str, is_secret: bool) -> Result<()> {
        Registry::set_variable(self, path, value, is_secret)
    }

    async fn list_variables(&self) -> Result<Vec<serde_json::Value>> {
        Registry::list_variables(self)
    }

    async fn get_resource(&self, path: &str) -> Result<Option<serde_json::Value>> {
        Registry::get_resource(self, path)
    }

    async fn set_resource(&self, path: &str, resource_type: &str, value: &serde_json::Value) -> Result<()> {
        Registry::set_resource(self, path, resource_type, value)
    }

    async fn list_resources(&self, resource_type: Option<&str>) -> Result<Vec<serde_json::Value>> {
        Registry::list_resources(self, resource_type)
    }

    async fn store_flow(&self, path: &str, version: &str, definition: &serde_json::Value, summary: Option<&str>, on_failure: Option<&str>) -> Result<String> {
        Registry::store_flow(self, path, version, definition, summary, on_failure)
    }

    async fn get_flow(&self, path: &str) -> Result<Option<serde_json::Value>> {
        Registry::get_flow(self, path)
    }

    async fn list_flows(&self) -> Result<Vec<serde_json::Value>> {
        Registry::list_flows(self)
    }

    async fn delete_flow(&self, path: &str) -> Result<()> {
        Registry::delete_flow(self, path)
    }

    async fn register_webhook(&self, target_url: &str, event: &str, secret: Option<&str>) -> Result<String> {
        Registry::register_webhook(self, target_url, event, secret)
    }

    async fn list_webhooks(&self, event: Option<&str>) -> Result<Vec<serde_json::Value>> {
        Registry::list_webhooks(self, event)
    }

    async fn delete_webhook(&self, id: &str) -> Result<()> {
        Registry::delete_webhook(self, id)
    }

    async fn store_execution(&self, execution: &FlowExecution) -> Result<()> {
        Registry::store_execution(self, execution)
    }

    async fn list_executions(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>> {
        Registry::list_executions(self, limit, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_registry() -> Registry {
        let dir = std::env::temp_dir().join(format!("automaton_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Registry::open(&dir).unwrap()
    }

    #[test]
    fn test_register_and_get_module() {
        let reg = test_registry();
        let mut manifest = AutomationManifest::default();
        manifest.name = "test.hello".to_string();
        manifest.version = "0.2.0".to_string();
        manifest.summary = Some("A test module".to_string());
        manifest.timeout_ms = 15_000;

        let id = reg.register("test.hello", "fn main() {}", &manifest).unwrap();
        assert_eq!(id.path, "test.hello");
        assert_eq!(id.version.to_string(), "0.2.0");

        let fetched = reg.get("test.hello").unwrap().expect("Module should exist");
        assert_eq!(fetched.manifest.name, "test.hello");
        assert_eq!(fetched.manifest.version, "0.2.0");
        assert_eq!(fetched.manifest.summary, Some("A test module".to_string()));
        assert!(!fetched.built);
    }

    #[test]
    fn test_list_modules() {
        let reg = test_registry();
        let m1 = AutomationManifest { name: "mod.a".to_string(), ..Default::default() };
        let m2 = AutomationManifest { name: "mod.b".to_string(), ..Default::default() };
        reg.register("mod.a", "// a", &m1).unwrap();
        reg.register("mod.b", "// b", &m2).unwrap();

        let list = reg.list().unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|(p, _, _, _)| p == "mod.a"));
        assert!(list.iter().any(|(p, _, _, _)| p == "mod.b"));
    }

    #[test]
    fn test_mark_built() {
        let reg = test_registry();
        let m = AutomationManifest { name: "test.built".to_string(), ..Default::default() };
        reg.register("test.built", "// src", &m).unwrap();
        reg.mark_built("test.built").unwrap();

        let fetched = reg.get("test.built").unwrap().expect("Module should exist");
        assert!(fetched.built);
    }

    #[test]
    fn test_variable_roundtrip() {
        let reg = test_registry();
        reg.set_variable("my/key", "hello", false).unwrap();
        let val = reg.get_variable("my/key").unwrap().expect("Var should exist");
        assert_eq!(val, "hello");
    }

    #[test]
    fn test_variable_list() {
        let reg = test_registry();
        reg.set_variable("var/a", "1", false).unwrap();
        reg.set_variable("var/b", "2", true).unwrap();
        let vars = reg.list_variables().unwrap();
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn test_resource_roundtrip() {
        let reg = test_registry();
        let val = serde_json::json!({"url": "https://example.com"});
        reg.set_resource("my/res", "http", &val).unwrap();
        let fetched = reg.get_resource("my/res").unwrap().expect("Resource should exist");
        assert_eq!(fetched["resource_type"], "http");
        assert_eq!(fetched["value"]["url"], "https://example.com");
    }

    #[test]
    fn test_list_resources() {
        let reg = test_registry();
        reg.set_resource("res/a", "http", &serde_json::json!({})).unwrap();
        reg.set_resource("res/b", "slack", &serde_json::json!({})).unwrap();
        let all = reg.list_resources(None).unwrap();
        assert_eq!(all.len(), 2);
        let filtered = reg.list_resources(Some("http")).unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_job_enqueue_dequeue_complete() {
        let reg = test_registry();
        let job_id = reg.enqueue("script", "test.module", &serde_json::json!({"x": 1})).unwrap();
        assert!(job_id > 0);

        let dequeued = reg.dequeue("worker1").unwrap().expect("Should dequeue");
        assert_eq!(dequeued["target_path"], "test.module");

        // Second dequeue should return None (already claimed)
        let second = reg.dequeue("worker2").unwrap();
        assert!(second.is_none());

        reg.complete_job(job_id).unwrap();
        let jobs = reg.list_jobs(10).unwrap();
        assert_eq!(jobs.len(), 0); // completed means deleted
    }

    #[test]
    fn test_list_jobs() {
        let reg = test_registry();
        reg.enqueue("flow", "flow.a", &serde_json::json!({})).unwrap();
        reg.enqueue("script", "mod.x", &serde_json::json!({})).unwrap();
        let jobs = reg.list_jobs(10).unwrap();
        assert_eq!(jobs.len(), 2);
    }

    #[test]
    fn test_trigger_create_and_get() {
        let reg = test_registry();
        let config = serde_json::json!({"schedule": "0 * * * *"});
        let id = reg.create_trigger("test.module", false, "cron", &config).unwrap();
        assert!(!id.is_empty());

        let triggers = reg.get_enabled_triggers("cron").unwrap();
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0]["target_path"], "test.module");

        // Non-matching type should return empty
        let webhooks = reg.get_enabled_triggers("webhook").unwrap();
        assert_eq!(webhooks.len(), 0);
    }

    #[test]
    fn test_flow_crud() {
        let reg = test_registry();
        let def = serde_json::json!([{"id": "step1", "kind": "Script"}]);

        let id = reg.store_flow("test.flow", "0.1.0", &def, Some("test"), None).unwrap();
        assert!(!id.is_empty());

        let fetched = reg.get_flow("test.flow").unwrap().expect("Flow should exist");
        assert_eq!(fetched["path"], "test.flow");

        let flows = reg.list_flows().unwrap();
        assert_eq!(flows.len(), 1);

        reg.delete_flow("test.flow").unwrap();
        let after = reg.get_flow("test.flow").unwrap();
        assert!(after.is_none());
    }

    #[test]
    fn test_run_record_and_query() {
        let reg = test_registry();
        reg.record_run("run-1", "test.module", &serde_json::json!({})).unwrap();
        reg.update_run("run-1", "completed", Some(&serde_json::json!({"ok": true})), None, 1).unwrap();

        let runs = reg.get_runs("test.module").unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["state"], "completed");
    }

    #[test]
    fn test_resolve_references_var() {
        let reg = test_registry();
        reg.set_variable("my/token", "abc123", false).unwrap();

        let input = serde_json::json!({"token": "$var:my/token"});
        let resolved = reg.resolve_references(&input).unwrap();
        assert_eq!(resolved["token"], "abc123");
    }

    #[test]
    fn test_resolve_references_res() {
        let reg = test_registry();
        let res_val = serde_json::json!({"host": "localhost", "port": 5432});
        reg.set_resource("my/db", "postgresql", &res_val).unwrap();

        let input = serde_json::json!({"db": "$res:my/db"});
        let resolved = reg.resolve_references(&input).unwrap();
        // get_resource returns { resource_type: ..., value: ... }
        assert_eq!(resolved["db"]["value"]["host"], "localhost");
        assert_eq!(resolved["db"]["value"]["port"], 5432);
    }
}
