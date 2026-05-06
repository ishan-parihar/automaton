use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use automaton_core::*;
use rusqlite::Connection;

/// Persistent property graph store backed by SQLite.
pub struct GraphStore {
    db: Mutex<Connection>,
}

fn with_db<T>(db: &Mutex<Connection>, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let conn = db
        .lock()
        .map_err(|e| AutomatonError::Database(e.to_string()))?;
    f(&conn)
}

fn with_db_mut<T>(db: &Mutex<Connection>, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let conn = db
        .lock()
        .map_err(|e| AutomatonError::Database(e.to_string()))?;
    f(&conn)
}

impl GraphStore {
    pub fn open(data_dir: &Path) -> Result<Self> {
        let db_path = data_dir.join("graph.db");
        let db = Connection::open(&db_path)?;
        let store = Self { db: Mutex::new(db) };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> Result<()> {
        with_db(&self.db, |db| {
            db.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS nodes (
                    id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    name TEXT NOT NULL,
                    properties TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS edges (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    target TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    properties TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    FOREIGN KEY (source) REFERENCES nodes(id),
                    FOREIGN KEY (target) REFERENCES nodes(id)
                );
                CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
                CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
                CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
                CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
                ",
            )?;
            Ok(())
        })
    }

    pub fn add_node(
        &self,
        kind: NodeKind,
        name: &str,
        properties: HashMap<String, serde_json::Value>,
    ) -> Result<String> {
        with_db(&self.db, |db| {
            let id = uuid::Uuid::new_v4().to_string();
            let kind_str = serde_json::to_string(&kind)?;
            let props_str = serde_json::to_string(&properties)?;
            db.execute(
                "INSERT INTO nodes (id, kind, name, properties) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, kind_str, name, props_str],
            )?;
            Ok(id)
        })
    }

    pub fn add_edge(
        &self,
        source: &str,
        target: &str,
        kind: EdgeKind,
        properties: HashMap<String, serde_json::Value>,
    ) -> Result<String> {
        with_db(&self.db, |db| {
            let id = uuid::Uuid::new_v4().to_string();
            let kind_str = serde_json::to_string(&kind)?;
            let props_str = serde_json::to_string(&properties)?;
            db.execute(
                "INSERT INTO edges (id, source, target, kind, properties) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, source, target, kind_str, props_str],
            )?;
            Ok(id)
        })
    }

    pub fn get_node(&self, id: &str) -> Result<Option<Node>> {
        with_db(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, kind, name, properties, created_at FROM nodes WHERE id = ?1",
            )?;
            let rows = stmt.query_map(rusqlite::params![id], row_to_node)?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result.into_iter().next())
        })
    }

    pub fn find_nodes_by_kind(&self, kind: NodeKind) -> Result<Vec<Node>> {
        with_db(&self.db, |db| {
            let kind_str = serde_json::to_string(&kind)?;
            let mut stmt = db.prepare(
                "SELECT id, kind, name, properties, created_at FROM nodes WHERE kind = ?1 ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map(rusqlite::params![kind_str], row_to_node)?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    pub fn get_outgoing_edges(&self, node_id: &str) -> Result<Vec<Edge>> {
        with_db(&self.db, |db| {
            let stmt = db.prepare(
                "SELECT id, source, target, kind, properties, created_at FROM edges WHERE source = ?1 ORDER BY created_at",
            )?;
            map_edges_query(stmt, rusqlite::params![node_id])
        })
    }

    pub fn get_incoming_edges(&self, node_id: &str) -> Result<Vec<Edge>> {
        with_db(&self.db, |db| {
            let stmt = db.prepare(
                "SELECT id, source, target, kind, properties, created_at FROM edges WHERE target = ?1 ORDER BY created_at",
            )?;
            map_edges_query(stmt, rusqlite::params![node_id])
        })
    }

    pub fn get_dependency_chain(&self, start_id: &str) -> Result<Vec<Node>> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let mut result = vec![];
        queue.push_back(start_id.to_string());
        while let Some(current) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            if let Some(node) = self.get_node(&current)? {
                result.push(node);
            }
            let edges = self.get_outgoing_edges(&current)?;
            for edge in edges {
                if matches!(edge.kind, EdgeKind::DependsOn) {
                    queue.push_back(edge.target);
                }
            }
        }
        Ok(result)
    }

    pub fn find_path(&self, from: &str, to: &str) -> Result<Vec<Vec<NodeAndEdge>>> {
        let all_edges = self.all_edges()?;
        let mut adj: HashMap<String, Vec<(String, EdgeKind)>> = HashMap::new();
        for edge in &all_edges {
            adj.entry(edge.source.clone())
                .or_default()
                .push((edge.target.clone(), edge.kind.clone()));
        }

        type StackEntry = (String, Vec<(String, EdgeKind)>, HashSet<String>);
        let mut paths: Vec<Vec<(String, EdgeKind)>> = vec![];
        let mut stack: Vec<StackEntry> = vec![(from.to_string(), vec![], HashSet::new())];

        while let Some((node, path_so_far, visited)) = stack.pop() {
            let mut visited = visited;
            if node == to {
                paths.push(path_so_far.clone());
                continue;
            }
            if !visited.insert(node.clone()) {
                continue;
            }
            if path_so_far.len() > 10 {
                continue;
            }
            if let Some(neighbors) = adj.get(&node) {
                for (next, kind) in neighbors {
                    let mut new_path = path_so_far.clone();
                    new_path.push((next.clone(), kind.clone()));
                    stack.push((next.clone(), new_path, visited.clone()));
                }
            }
        }

        let mut result = vec![];
        for raw_path in &paths {
            let mut segment = vec![];
            for (node_id, edge_kind) in raw_path {
                if let Some(node) = self.get_node(node_id)? {
                    segment.push(NodeAndEdge {
                        node,
                        edge_kind: edge_kind.clone(),
                    });
                }
            }
            result.push(segment);
        }
        Ok(result)
    }

    pub fn all_edges(&self) -> Result<Vec<Edge>> {
        with_db(&self.db, |db| {
            let stmt = db.prepare(
                "SELECT id, source, target, kind, properties, created_at FROM edges ORDER BY created_at",
            )?;
            map_edges_query(stmt, [])
        })
    }

    pub fn all_nodes(&self) -> Result<Vec<Node>> {
        with_db(&self.db, |db| {
            let mut stmt = db.prepare(
                "SELECT id, kind, name, properties, created_at FROM nodes ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], row_to_node)?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    pub fn delete_node(&self, id: &str) -> Result<()> {
        with_db(&self.db, |db| {
            db.execute(
                "DELETE FROM edges WHERE source = ?1 OR target = ?1",
                rusqlite::params![id],
            )?;
            db.execute("DELETE FROM nodes WHERE id = ?1", rusqlite::params![id])?;
            Ok(())
        })
    }

    pub fn delete_edge(&self, id: &str) -> Result<()> {
        with_db(&self.db, |db| {
            db.execute("DELETE FROM edges WHERE id = ?1", rusqlite::params![id])?;
            Ok(())
        })
    }
}

fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<Node> {
    let props_str: String = row.get(3)?;
    Ok(Node {
        id: row.get(0)?,
        kind: serde_json::from_str(&row.get::<_, String>(1)?).unwrap_or(NodeKind::Module),
        name: row.get(2)?,
        properties: serde_json::from_str(&props_str).unwrap_or_default(),
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
            .map(|dt| dt.to_utc())
            .unwrap_or_default(),
    })
}

fn map_edges_query(
    mut stmt: rusqlite::Statement,
    params: impl rusqlite::Params,
) -> Result<Vec<Edge>> {
    let rows = stmt.query_map(params, |row| {
        let props_str: String = row.get(4)?;
        Ok(Edge {
            id: row.get(0)?,
            source: row.get(1)?,
            target: row.get(2)?,
            kind: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(EdgeKind::DependsOn),
            properties: serde_json::from_str(&props_str).unwrap_or_default(),
            created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                .map(|dt| dt.to_utc())
                .unwrap_or_default(),
        })
    })?;
    let mut result = vec![];
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

#[derive(Debug, Clone)]
pub struct NodeAndEdge {
    pub node: Node,
    pub edge_kind: EdgeKind,
}
