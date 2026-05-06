use std::path::{Path, PathBuf};
use std::sync::Mutex;

use automaton_core::*;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

/// Local module registry backed by SQLite.
pub struct Registry {
    db: Mutex<Connection>,
    data_dir: PathBuf,
    build_cache: PathBuf,
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
        let registry = Self {
            db: Mutex::new(db),
            data_dir: data_dir.to_path_buf(),
            build_cache,
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
                    value TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
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
}
