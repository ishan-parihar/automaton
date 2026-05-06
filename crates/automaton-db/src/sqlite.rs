//! SQLite backend for DbPool using rusqlite.
//! Uses direct mutex locking (no closure helpers) to avoid async lifetime issues.

use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;
use serde_json::Value;

use crate::models::*;
use crate::DbPool;

pub struct SqlitePool {
    db: Mutex<Connection>,
}

impl SqlitePool {
    pub fn open(data_dir: &Path) -> Result<Self, String> {
        let db_path = data_dir.join("registry.db");
        std::fs::create_dir_all(data_dir).map_err(|e| format!("Create dir: {e}"))?;
        let conn = Connection::open(&db_path).map_err(|e| format!("Open DB: {e}"))?;
        let pool = Self { db: Mutex::new(conn) };
        pool.migrate()?;
        Ok(pool)
    }

    fn migrate(&self) -> Result<(), String> {
        let conn = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scripts (
                hash TEXT PRIMARY KEY, path TEXT NOT NULL, version TEXT NOT NULL DEFAULT '0.1.0',
                parent_hash TEXT REFERENCES scripts(hash), source TEXT NOT NULL,
                manifest TEXT NOT NULL DEFAULT '{}', built INTEGER NOT NULL DEFAULT 0,
                language TEXT NOT NULL DEFAULT 'rust', created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS script_deps (
                script_hash TEXT NOT NULL REFERENCES scripts(hash) ON DELETE CASCADE,
                depends_on TEXT NOT NULL, version_req TEXT, PRIMARY KEY (script_hash, depends_on)
            );
            CREATE TABLE IF NOT EXISTS jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL DEFAULT 'script',
                target_path TEXT NOT NULL, args TEXT NOT NULL DEFAULT '{}',
                scheduled_for TEXT NOT NULL DEFAULT (datetime('now')), priority INTEGER NOT NULL DEFAULT 0,
                running INTEGER NOT NULL DEFAULT 0, worker_id TEXT,
                max_attempts INTEGER NOT NULL DEFAULT 3, attempt INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY, target_path TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'script',
                args TEXT NOT NULL DEFAULT '{}', result TEXT, error TEXT,
                state TEXT NOT NULL DEFAULT 'pending', attempt INTEGER NOT NULL DEFAULT 1,
                duration_ms INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')),
                completed_at TEXT
            );
            CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL,
                properties TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS graph_edges (
                id TEXT PRIMARY KEY, source TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                target TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                kind TEXT NOT NULL, properties TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS variables (
                path TEXT PRIMARY KEY, value TEXT NOT NULL,
                is_secret INTEGER NOT NULL DEFAULT 1, description TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS resources (
                path TEXT PRIMARY KEY, resource_type TEXT NOT NULL,
                value TEXT NOT NULL DEFAULT '{}', description TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS triggers (
                id TEXT PRIMARY KEY, target_path TEXT NOT NULL,
                target_is_flow INTEGER NOT NULL DEFAULT 0, trigger_type TEXT NOT NULL DEFAULT 'cron',
                config TEXT NOT NULL DEFAULT '{}', enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        ).map_err(|e| format!("Migration: {e}"))?;
        Ok(())
    }
}

macro_rules! with_db {
    ($self:expr, |$conn:ident| $($body:tt)*) => {{
        let __conn = match $self.db.lock() {
            Ok(c) => c,
            Err(e) => return Err(format!("Lock: {e}")),
        };
        let $conn = &*__conn;
        let __r: std::result::Result<_, String> = { $($body)* };
        __r
    }};
}

#[async_trait]
impl DbPool for SqlitePool {
    async fn register_script(&self, path: &str, source: &str, _version: &str, manifest: &Value, deps: &[automaton_core::DepRef]) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        let manifest_s = serde_json::to_string(manifest).map_err(|e| format!("Ser: {e}"))?;
        let deps_owned: Vec<(String, Option<String>)> = deps.iter().map(|d| (d.name.clone(), d.version_req.clone())).collect();
        let p = path.to_string();
        let s = source.to_string();
        let h = hash.clone();
        with_db!(self, |conn| {
            conn.execute(
                "INSERT INTO scripts (hash, path, version, source, manifest) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(hash) DO UPDATE SET path=excluded.path",
                rusqlite::params![&h, &p, "0.1.0", &s, &manifest_s],
            ).map_err(|e| format!("Insert: {e}"))?;
            for (dn, dv) in &deps_owned {
                conn.execute(
                    "INSERT INTO script_deps (script_hash, depends_on, version_req) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
                    rusqlite::params![&h, dn, dv],
                ).map_err(|e| format!("Dep: {e}"))?;
            }
            Ok(h)
        })
    }

    async fn get_script(&self, path: &str) -> Result<Option<ScriptRecord>, String> {
        let p = path.to_string();
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT hash, path, version, source, manifest, built, created_at FROM scripts WHERE path = ?1 ORDER BY created_at DESC LIMIT 1")
                .map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query(rusqlite::params![p]).map_err(|e| format!("Q: {e}"))?;
            match rows.next().map_err(|e| format!("N: {e}"))? {
                Some(row) => Ok(Some(ScriptRecord {
                    hash: row.get::<_, String>(0).map_err(|e| format!("h: {e}"))?,
                    path: row.get::<_, String>(1).map_err(|e| format!("p: {e}"))?,
                    version: row.get::<_, String>(2).map_err(|e| format!("v: {e}"))?,
                    source: row.get::<_, String>(3).map_err(|e| format!("s: {e}"))?,
                    manifest: serde_json::from_str(&row.get::<_, String>(4).map_err(|e| format!("m: {e}"))?).unwrap_or_default(),
                    built: row.get::<_, bool>(5).map_err(|e| format!("b: {e}"))?,
                    created_at: row.get::<_, String>(6).map_err(|e| format!("c: {e}"))?,
                })),
                None => Ok(None),
            }
        })
    }

    async fn list_scripts(&self) -> Result<Vec<ScriptRecord>, String> {
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT hash, path, version, source, manifest, built, created_at FROM scripts ORDER BY path")
                .map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query([]).map_err(|e| format!("Q: {e}"))?;
            let mut r = vec![];
            while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                r.push(ScriptRecord {
                    hash: row.get::<_, String>(0).unwrap_or_default(),
                    path: row.get::<_, String>(1).unwrap_or_default(),
                    version: row.get::<_, String>(2).unwrap_or_default(),
                    source: row.get::<_, String>(3).unwrap_or_default(),
                    manifest: serde_json::from_str(&row.get::<_, String>(4).unwrap_or_default()).unwrap_or_default(),
                    built: row.get::<_, bool>(5).unwrap_or_default(),
                    created_at: row.get::<_, String>(6).unwrap_or_default(),
                });
            }
            Ok(r)
        })
    }

    async fn mark_built(&self, path: &str) -> Result<(), String> {
        let p = path.to_string();
        with_db!(self, |conn| {
            conn.execute("UPDATE scripts SET built = 1 WHERE path = ?1", rusqlite::params![p])
                .map_err(|e| format!("Upd: {e}"))?;
            Ok(())
        })
    }

    async fn enqueue(&self, kind: &str, target: &str, args: &Value) -> Result<i64, String> {
        let k = kind.to_string(); let t = target.to_string(); let a = serde_json::to_string(args).map_err(|e| format!("Ser: {e}"))?;
        with_db!(self, |conn| {
            conn.execute("INSERT INTO jobs (kind, target_path, args) VALUES (?1, ?2, ?3)", rusqlite::params![k, t, a])
                .map_err(|e| format!("Ins: {e}"))?;
            Ok(conn.last_insert_rowid())
        })
    }

    async fn dequeue(&self, worker_id: &str) -> Result<Option<JobRecord>, String> {
        let wid = worker_id.to_string();
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT id, kind, target_path, args, scheduled_for, priority FROM jobs WHERE NOT running AND scheduled_for <= datetime('now') ORDER BY priority DESC, scheduled_for ASC LIMIT 1")
                .map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query([]).map_err(|e| format!("Q: {e}"))?;
            if let Some(r) = rows.next().map_err(|e| format!("N: {e}"))? {
                let id: i64 = r.get(0).map_err(|e| format!("id: {e}"))?;
                let kind: String = r.get(1).map_err(|e| format!("k: {e}"))?;
                let target_path: String = r.get(2).map_err(|e| format!("t: {e}"))?;
                let args_s: String = r.get(3).map_err(|e| format!("a: {e}"))?;
                let sched: String = r.get(4).map_err(|e| format!("s: {e}"))?;
                let prio: i32 = r.get(5).map_err(|e| format!("p: {e}"))?;
                conn.execute("UPDATE jobs SET running = 1, worker_id = ?1 WHERE id = ?2", rusqlite::params![wid, id])
                    .map_err(|e| format!("Upd: {e}"))?;
                Ok(Some(JobRecord {
                    id, kind, target_path,
                    args: serde_json::from_str(&args_s).unwrap_or_default(),
                    scheduled_for: sched, priority: prio,
                }))
            } else {
                Ok(None)
            }
        })
    }

    async fn complete_job(&self, job_id: i64) -> Result<(), String> {
        with_db!(self, |conn| {
            conn.execute("DELETE FROM jobs WHERE id = ?1", rusqlite::params![job_id])
                .map_err(|e| format!("Del: {e}"))?;
            Ok(())
        })
    }

    async fn record_run(&self, id: &str, target: &str, kind: &str, args: &Value) -> Result<(), String> {
        let i = id.to_string(); let t = target.to_string(); let k = kind.to_string();
        let a = serde_json::to_string(args).map_err(|e| format!("Ser: {e}"))?;
        with_db!(self, |conn| {
            conn.execute("INSERT INTO runs (id, target_path, kind, args) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![i, t, k, a])
                .map_err(|e| format!("Ins: {e}"))?;
            Ok(())
        })
    }

    async fn update_run(&self, id: &str, state: &str, result: Option<&Value>, error: Option<&str>, attempt: u32, duration_ms: i64) -> Result<(), String> {
        let i = id.to_string(); let s = state.to_string();
        let r = result.map(|v| serde_json::to_string(v).unwrap_or_default());
        let e = error.map(|s| s.to_string());
        with_db!(self, |conn| {
            conn.execute("UPDATE runs SET state = ?1, result = ?2, error = ?3, attempt = ?4, duration_ms = ?5, completed_at = datetime('now') WHERE id = ?6",
                rusqlite::params![s, r, e, attempt as i32, duration_ms, i]).map_err(|e| format!("Upd: {e}"))?;
            Ok(())
        })
    }

    async fn get_runs(&self, target: &str, limit: i64) -> Result<Vec<RunRecord>, String> {
        let t = target.to_string();
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT id, target_path, state, attempt, error, duration_ms, created_at FROM runs WHERE target_path = ?1 ORDER BY created_at DESC LIMIT ?2")
                .map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query(rusqlite::params![t, limit]).map_err(|e| format!("Q: {e}"))?;
            let mut r = vec![];
            while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                r.push(RunRecord {
                    id: row.get::<_, String>(0).unwrap_or_default(),
                    target_path: row.get::<_, String>(1).unwrap_or_default(),
                    state: row.get::<_, String>(2).unwrap_or_default(),
                    attempt: row.get::<_, i32>(3).unwrap_or_default(),
                    error: row.get::<_, Option<String>>(4).unwrap_or_default(),
                    duration_ms: row.get::<_, i64>(5).unwrap_or_default(),
                    created_at: row.get::<_, String>(6).unwrap_or_default(),
                });
            }
            Ok(r)
        })
    }

    async fn add_node(&self, kind: &str, name: &str, props: &Value) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let i = id.clone(); let k = kind.to_string(); let n = name.to_string();
        let p = serde_json::to_string(props).map_err(|e| format!("Ser: {e}"))?;
        with_db!(self, |conn| {
            conn.execute("INSERT INTO graph_nodes (id, kind, name, properties) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![i, k, n, p])
                .map_err(|e| format!("Ins: {e}"))?;
            Ok(id)
        })
    }

    async fn add_edge(&self, source: &str, target: &str, kind: &str) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let i = id.clone(); let s = source.to_string(); let t = target.to_string(); let k = kind.to_string();
        with_db!(self, |conn| {
            conn.execute("INSERT INTO graph_edges (id, source, target, kind) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![i, s, t, k])
                .map_err(|e| format!("Ins: {e}"))?;
            Ok(id)
        })
    }

    async fn get_nodes(&self, kind: Option<&str>) -> Result<Vec<NodeRecord>, String> {
        let kind_owned = kind.map(|s| s.to_string());
        with_db!(self, |conn| {
            let mut result = vec![];
            if let Some(ref k) = kind_owned {
                let mut stmt = conn.prepare("SELECT id, kind, name, properties, created_at FROM graph_nodes WHERE kind = ?1 ORDER BY created_at")
                    .map_err(|e| format!("Prep: {e}"))?;
                let mut rows = stmt.query(rusqlite::params![k]).map_err(|e| format!("Q: {e}"))?;
                while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                    result.push(row_to_node(row).map_err(|e| format!("R: {e}"))?);
                }
            } else {
                let mut stmt = conn.prepare("SELECT id, kind, name, properties, created_at FROM graph_nodes ORDER BY created_at")
                    .map_err(|e| format!("Prep: {e}"))?;
                let mut rows = stmt.query([]).map_err(|e| format!("Q: {e}"))?;
                while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                    result.push(row_to_node(row).map_err(|e| format!("R: {e}"))?);
                }
            }
            Ok(result)
        })
    }

    async fn get_edges(&self) -> Result<Vec<EdgeRecord>, String> {
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT id, source, target, kind, properties, created_at FROM graph_edges ORDER BY created_at")
                .map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query([]).map_err(|e| format!("Q: {e}"))?;
            let mut r = vec![];
            while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                r.push(EdgeRecord {
                    id: row.get::<_, String>(0).unwrap_or_default(),
                    source: row.get::<_, String>(1).unwrap_or_default(),
                    target: row.get::<_, String>(2).unwrap_or_default(),
                    kind: row.get::<_, String>(3).unwrap_or_default(),
                    properties: serde_json::from_str(&row.get::<_, String>(4).unwrap_or_default()).unwrap_or_default(),
                    created_at: row.get::<_, String>(5).unwrap_or_default(),
                });
            }
            Ok(r)
        })
    }

    async fn set_variable(&self, path: &str, value: &str, is_secret: bool) -> Result<(), String> {
        let p = path.to_string(); let v = value.to_string();
        with_db!(self, |conn| {
            conn.execute("INSERT INTO variables (path, value, is_secret) VALUES (?1, ?2, ?3) ON CONFLICT(path) DO UPDATE SET value=excluded.value, is_secret=excluded.is_secret",
                rusqlite::params![p, v, is_secret as i32]).map_err(|e| format!("Ins: {e}"))?;
            Ok(())
        })
    }

    async fn get_variable(&self, path: &str) -> Result<Option<String>, String> {
        let p = path.to_string();
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT value FROM variables WHERE path = ?1").map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query(rusqlite::params![p]).map_err(|e| format!("Q: {e}"))?;
            match rows.next().map_err(|e| format!("N: {e}"))? {
                Some(row) => Ok(Some(row.get::<_, String>(0).map_err(|e| format!("v: {e}"))?)),
                None => Ok(None),
            }
        })
    }

    async fn list_variables(&self) -> Result<Vec<Value>, String> {
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT path, is_secret, description FROM variables ORDER BY path").map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query([]).map_err(|e| format!("Q: {e}"))?;
            let mut r = vec![];
            while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                r.push(serde_json::json!({
                    "path": row.get::<_, String>(0).unwrap_or_default(),
                    "is_secret": row.get::<_, bool>(1).unwrap_or_default(),
                    "description": row.get::<_, Option<String>>(2).unwrap_or_default(),
                }));
            }
            Ok(r)
        })
    }

    async fn set_resource(&self, path: &str, rtype: &str, value: &Value) -> Result<(), String> {
        let p = path.to_string(); let t = rtype.to_string();
        let v = serde_json::to_string(value).map_err(|e| format!("Ser: {e}"))?;
        with_db!(self, |conn| {
            conn.execute("INSERT INTO resources (path, resource_type, value) VALUES (?1, ?2, ?3) ON CONFLICT(path) DO UPDATE SET resource_type=excluded.resource_type, value=excluded.value",
                rusqlite::params![p, t, v]).map_err(|e| format!("Ins: {e}"))?;
            Ok(())
        })
    }

    async fn get_resource(&self, path: &str) -> Result<Option<Value>, String> {
        let p = path.to_string();
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT resource_type, value FROM resources WHERE path = ?1").map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query(rusqlite::params![p]).map_err(|e| format!("Q: {e}"))?;
            match rows.next().map_err(|e| format!("N: {e}"))? {
                Some(row) => Ok(Some(serde_json::json!({
                    "type": row.get::<_, String>(0).map_err(|e| format!("t: {e}"))?,
                    "value": serde_json::from_str::<Value>(&row.get::<_, String>(1).map_err(|e| format!("v: {e}"))?).unwrap_or_default(),
                }))),
                None => Ok(None),
            }
        })
    }

    async fn list_resources(&self, rtype: Option<&str>) -> Result<Vec<Value>, String> {
        let rt = rtype.map(|s| s.to_string());
        with_db!(self, |conn| {
            let mut result = vec![];
            if let Some(ref t) = rt {
                let mut stmt = conn.prepare("SELECT path, resource_type FROM resources WHERE resource_type = ?1 ORDER BY path")
                    .map_err(|e| format!("Prep: {e}"))?;
                let mut rows = stmt.query(rusqlite::params![t]).map_err(|e| format!("Q: {e}"))?;
                while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                    result.push(serde_json::json!({
                        "path": row.get::<_, String>(0).unwrap_or_default(),
                        "type": row.get::<_, String>(1).unwrap_or_default(),
                    }));
                }
            } else {
                let mut stmt = conn.prepare("SELECT path, resource_type FROM resources ORDER BY path")
                    .map_err(|e| format!("Prep: {e}"))?;
                let mut rows = stmt.query([]).map_err(|e| format!("Q: {e}"))?;
                while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                    result.push(serde_json::json!({
                        "path": row.get::<_, String>(0).unwrap_or_default(),
                        "type": row.get::<_, String>(1).unwrap_or_default(),
                    }));
                }
            }
            Ok(result)
        })
    }

    async fn create_trigger(&self, target: &str, is_flow: bool, ttype: &str, config: &Value) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let i = id.clone(); let t = target.to_string(); let tp = ttype.to_string();
        let c = serde_json::to_string(config).map_err(|e| format!("Ser: {e}"))?;
        with_db!(self, |conn| {
            conn.execute("INSERT INTO triggers (id, target_path, target_is_flow, trigger_type, config) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![i, t, is_flow as i32, tp, c]).map_err(|e| format!("Ins: {e}"))?;
            Ok(id)
        })
    }

    async fn get_enabled_triggers(&self, ttype: &str) -> Result<Vec<TriggerRecord>, String> {
        let tp = ttype.to_string();
        with_db!(self, |conn| {
            let mut stmt = conn.prepare("SELECT id, target_path, target_is_flow, config, created_at FROM triggers WHERE enabled AND trigger_type = ?1")
                .map_err(|e| format!("Prep: {e}"))?;
            let mut rows = stmt.query(rusqlite::params![tp]).map_err(|e| format!("Q: {e}"))?;
            let mut r = vec![];
            while let Some(row) = rows.next().map_err(|e| format!("N: {e}"))? {
                r.push(TriggerRecord {
                    id: row.get::<_, String>(0).unwrap_or_default(),
                    target_path: row.get::<_, String>(1).unwrap_or_default(),
                    target_is_flow: row.get::<_, bool>(2).unwrap_or_default(),
                    config: serde_json::from_str(&row.get::<_, String>(3).unwrap_or_default()).unwrap_or_default(),
                    created_at: row.get::<_, String>(4).unwrap_or_default(),
                });
            }
            Ok(r)
        })
    }
}

fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<NodeRecord> {
    Ok(NodeRecord {
        id: row.get::<_, String>(0)?, kind: row.get::<_, String>(1)?, name: row.get::<_, String>(2)?,
        properties: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
        created_at: row.get::<_, String>(4)?,
    })
}
