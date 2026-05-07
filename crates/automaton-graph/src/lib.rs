use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use automaton_core::*;

use rusqlite::Connection;
use serde::Serialize;

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

impl GraphStore {
    /// Open the graph store from the dedicated graph.db file.
    /// Kept for backward compatibility. Prefer `open_merged` for new installations.
    pub fn open(data_dir: &Path) -> Result<Self> {
        let db_path = data_dir.join("graph.db");
        let db = Connection::open(&db_path)?;
        let store = Self { db: Mutex::new(db) };
        store.init_tables()?;
        Ok(store)
    }

    /// Open the graph store using registry.db, which now contains merged
    /// graph_nodes and graph_edges tables alongside all registry tables.
    /// This eliminates the separate graph.db file.
    pub fn open_merged(data_dir: &Path) -> Result<Self> {
        let db_path = data_dir.join("registry.db");
        let db = Connection::open(&db_path)?;
        Ok(Self { db: Mutex::new(db) })
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
                CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
                CREATE INDEX IF NOT EXISTS idx_nodes_created_at ON nodes(created_at);
                CREATE INDEX IF NOT EXISTS idx_edges_created_at ON edges(created_at);
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

    pub fn find_nodes_by_kind_paginated(
        &self,
        kind: NodeKind,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Node>> {
        with_db(&self.db, |db| {
            let kind_str = serde_json::to_string(&kind)?;
            let mut sql = String::from(
                "SELECT id, kind, name, properties, created_at FROM nodes WHERE kind = ? ORDER BY created_at DESC",
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];
            params.push(Box::new(kind_str));
            if let Some(l) = limit {
                sql.push_str(" LIMIT ?");
                params.push(Box::new(l as i64));
            }
            if let Some(o) = offset {
                sql.push_str(" OFFSET ?");
                params.push(Box::new(o as i64));
            }
            let mut stmt = db.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), row_to_node)?;
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
            // Add the start node (not included in DFS path traversal)
            if let Some(node) = self.get_node(from)? {
                segment.push(NodeAndEdge {
                    node,
                    edge_kind: raw_path.first().map(|(_, ek)| ek.clone()).unwrap_or(EdgeKind::DependsOn),
                });
            }
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

    pub fn all_nodes_paginated(&self, limit: Option<u32>, offset: Option<u32>) -> Result<Vec<Node>> {
        with_db(&self.db, |db| {
            let mut sql = String::from(
                "SELECT id, kind, name, properties, created_at FROM nodes ORDER BY created_at",
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];
            if let Some(l) = limit {
                sql.push_str(" LIMIT ?");
                params.push(Box::new(l as i64));
            }
            if let Some(o) = offset {
                sql.push_str(" OFFSET ?");
                params.push(Box::new(o as i64));
            }
            let mut stmt = db.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), row_to_node)?;
            let mut result = vec![];
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    /// Find all nodes whose properties match all the given key-value pairs.
    /// Uses `json_extract()` SQL for efficient server-side filtering instead of
    /// loading all nodes and scanning in memory.
    pub fn find_nodes_by_properties(
        &self,
        properties: &HashMap<String, serde_json::Value>,
    ) -> Result<Vec<NodeAndEdge>> {
        with_db(&self.db, |conn| {
            let mut sql = String::from(
                "SELECT id, kind, name, properties, created_at FROM nodes WHERE 1=1",
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];

            for (key, value) in properties {
                let key_clean = key.replace('"', "\"\"");
                sql.push_str(&format!(
                    " AND json_extract(properties, '$.\"{}\"') = json(?)",
                    key_clean
                ));
                params.push(Box::new(serde_json::to_string(value)?));
            }

            sql.push_str(" ORDER BY created_at DESC");

            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), row_to_node)?;
            let mut result = vec![];
            for row in rows {
                let node = row?;
                result.push(NodeAndEdge {
                    node,
                    edge_kind: EdgeKind::DependsOn,
                });
            }
            Ok(result)
        })
    }

    /// Paginated variant of `find_nodes_by_properties`.
    /// Uses `json_extract()` SQL with LIMIT/OFFSET for efficient server-side pagination.
    pub fn find_nodes_by_properties_paginated(
        &self,
        properties: &HashMap<String, serde_json::Value>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<NodeAndEdge>> {
        with_db(&self.db, |conn| {
            let mut sql = String::from(
                "SELECT id, kind, name, properties, created_at FROM nodes WHERE 1=1",
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];

            for (key, value) in properties {
                let key_clean = key.replace('"', "\"\"");
                sql.push_str(&format!(
                    " AND json_extract(properties, '$.\"{}\"') = json(?)",
                    key_clean
                ));
                params.push(Box::new(serde_json::to_string(value)?));
            }

            sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");
            params.push(Box::new(limit as i64));
            params.push(Box::new(offset as i64));

            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), row_to_node)?;
            let mut result = vec![];
            for row in rows {
                let node = row?;
                result.push(NodeAndEdge {
                    node,
                    edge_kind: EdgeKind::DependsOn,
                });
            }
            Ok(result)
        })
    }

    /// Search nodes by name using a LIKE query (case-insensitive substring match).
    pub fn search_nodes(&self, query: &str) -> Result<Vec<NodeAndEdge>> {
        with_db(&self.db, |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, kind, name, properties, created_at FROM nodes WHERE name LIKE '%' || ?1 || '%' ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map(rusqlite::params![query], row_to_node)?;
            let mut result = vec![];
            for row in rows {
                let node = row?;
                result.push(NodeAndEdge {
                    node,
                    edge_kind: EdgeKind::DependsOn,
                });
            }
            Ok(result)
        })
    }

    /// Find nodes created within the given time range.
    /// `start` and `end` are ISO 8601 datetime strings (inclusive).
    /// Pass `None` for unbounded range on either side.
    pub fn find_nodes_in_time_range(
        &self,
        start: Option<&str>,
        end: Option<&str>,
    ) -> Result<Vec<NodeAndEdge>> {
        with_db(&self.db, |conn| {
            let mut sql = String::from(
                "SELECT id, kind, name, properties, created_at FROM nodes WHERE 1=1",
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];

            if let Some(start) = start {
                sql.push_str(" AND created_at >= ?");
                params.push(Box::new(start.to_string()));
            }
            if let Some(end) = end {
                sql.push_str(" AND created_at <= ?");
                params.push(Box::new(end.to_string()));
            }

            sql.push_str(" ORDER BY created_at DESC");

            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), row_to_node)?;
            let mut result = vec![];
            for row in rows {
                let node = row?;
                result.push(NodeAndEdge {
                    node,
                    edge_kind: EdgeKind::DependsOn,
                });
            }
            Ok(result)
        })
    }

    /// Find edges created within the given time range and return their source nodes.
    /// `start` and `end` are ISO 8601 datetime strings (inclusive).
    /// Pass `None` for unbounded range on either side.
    pub fn find_edges_in_time_range(
        &self,
        start: Option<&str>,
        end: Option<&str>,
    ) -> Result<Vec<NodeAndEdge>> {
        with_db(&self.db, |conn| {
            let mut sql = String::from(
                "SELECT n.id, n.kind, n.name, n.properties, n.created_at, e.kind \
                 FROM edges e \
                 JOIN nodes n ON n.id = e.source \
                 WHERE 1=1",
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];

            if let Some(start) = start {
                sql.push_str(" AND e.created_at >= ?");
                params.push(Box::new(start.to_string()));
            }
            if let Some(end) = end {
                sql.push_str(" AND e.created_at <= ?");
                params.push(Box::new(end.to_string()));
            }

            sql.push_str(" ORDER BY e.created_at DESC");

            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                let props_str: String = row.get(3)?;
                let node = Node {
                    id: row.get(0)?,
                    kind: serde_json::from_str(&row.get::<_, String>(1)?)
                        .unwrap_or(NodeKind::Module),
                    name: row.get(2)?,
                    properties: serde_json::from_str(&props_str).unwrap_or_default(),
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .map(|dt| dt.to_utc())
                        .unwrap_or_default(),
                };
                let edge_kind: EdgeKind =
                    serde_json::from_str(&row.get::<_, String>(5)?)
                        .unwrap_or(EdgeKind::DependsOn);
                Ok(NodeAndEdge { node, edge_kind })
            })?;
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

    /// Get aggregated summary statistics about the graph.
    pub fn summarize(&self) -> Result<GraphSummary> {
        with_db(&self.db, |db| {
            let total_nodes: u64 = db.query_row("SELECT COUNT(*) FROM nodes", [], |row| {
                row.get(0)
            })?;
            let total_edges: u64 = db.query_row("SELECT COUNT(*) FROM edges", [], |row| {
                row.get(0)
            })?;

            let mut nodes_by_kind: HashMap<String, u64> = HashMap::new();
            let mut stmt = db.prepare("SELECT kind, COUNT(*) as cnt FROM nodes GROUP BY kind")?;
            let rows = stmt.query_map([], |row| {
                let kind: String = row.get(0)?;
                let cnt: u64 = row.get(1)?;
                Ok((kind.trim_matches('"').to_string(), cnt))
            })?;
            for row in rows {
                let (kind, cnt) = row?;
                nodes_by_kind.insert(kind, cnt);
            }

            let mut edges_by_kind: HashMap<String, u64> = HashMap::new();
            let mut stmt = db.prepare("SELECT kind, COUNT(*) as cnt FROM edges GROUP BY kind")?;
            let rows = stmt.query_map([], |row| {
                let kind: String = row.get(0)?;
                let cnt: u64 = row.get(1)?;
                Ok((kind.trim_matches('"').to_string(), cnt))
            })?;
            for row in rows {
                let (kind, cnt) = row?;
                edges_by_kind.insert(kind, cnt);
            }

            Ok(GraphSummary {
                total_nodes,
                total_edges,
                nodes_by_kind,
                edges_by_kind,
            })
        })
    }
}

/// Summary statistics about the knowledge graph.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphSummary {
    pub total_nodes: u64,
    pub total_edges: u64,
    pub nodes_by_kind: HashMap<String, u64>,
    pub edges_by_kind: HashMap<String, u64>,
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

#[derive(Debug, Clone, Serialize)]
pub struct NodeAndEdge {
    pub node: Node,
    pub edge_kind: EdgeKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_store() -> GraphStore {
        let dir =
            std::env::temp_dir().join(format!("automaton_graph_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        GraphStore::open(&dir).unwrap()
    }

    #[test]
    fn test_add_and_get_node() {
        let store = test_store();
        let mut props = HashMap::new();
        props.insert("key".to_string(), serde_json::json!("value"));
        let id = store
            .add_node(NodeKind::Module, "test.module", props)
            .unwrap();
        assert!(!id.is_empty());

        let node = store.get_node(&id).unwrap().expect("Node should exist");
        assert_eq!(node.name, "test.module");
        assert_eq!(node.kind, NodeKind::Module);
    }

    #[test]
    fn test_add_and_get_edge() {
        let store = test_store();
        let a = store
            .add_node(NodeKind::Module, "mod.a", HashMap::new())
            .unwrap();
        let b = store
            .add_node(NodeKind::Module, "mod.b", HashMap::new())
            .unwrap();
        let eid = store
            .add_edge(&a, &b, EdgeKind::DependsOn, HashMap::new())
            .unwrap();
        assert!(!eid.is_empty());

        let outgoing = store.get_outgoing_edges(&a).unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].target, b);

        let incoming = store.get_incoming_edges(&b).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source, a);
    }

    #[test]
    fn test_find_nodes_by_kind() {
        let store = test_store();
        store
            .add_node(NodeKind::Module, "mod.a", HashMap::new())
            .unwrap();
        store
            .add_node(NodeKind::Trigger, "trig.x", HashMap::new())
            .unwrap();
        store
            .add_node(NodeKind::Workflow, "flow.y", HashMap::new())
            .unwrap();

        let modules = store.find_nodes_by_kind(NodeKind::Module).unwrap();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "mod.a");
    }

    #[test]
    fn test_delete_node() {
        let store = test_store();
        let id = store
            .add_node(NodeKind::Module, "to_delete", HashMap::new())
            .unwrap();
        store.delete_node(&id).unwrap();
        assert!(store.get_node(&id).unwrap().is_none());
    }

    #[test]
    fn test_delete_edge() {
        let store = test_store();
        let a = store
            .add_node(NodeKind::Module, "a", HashMap::new())
            .unwrap();
        let b = store
            .add_node(NodeKind::Module, "b", HashMap::new())
            .unwrap();
        let eid = store
            .add_edge(&a, &b, EdgeKind::Calls, HashMap::new())
            .unwrap();
        store.delete_edge(&eid).unwrap();
        assert_eq!(store.get_outgoing_edges(&a).unwrap().len(), 0);
    }

    #[test]
    fn test_dependency_chain() {
        let store = test_store();
        let a = store
            .add_node(NodeKind::Module, "root", HashMap::new())
            .unwrap();
        let b = store
            .add_node(NodeKind::Module, "dep1", HashMap::new())
            .unwrap();
        let c = store
            .add_node(NodeKind::Module, "dep2", HashMap::new())
            .unwrap();
        store
            .add_edge(&a, &b, EdgeKind::DependsOn, HashMap::new())
            .unwrap();
        store
            .add_edge(&b, &c, EdgeKind::DependsOn, HashMap::new())
            .unwrap();

        let chain = store.get_dependency_chain(&a).unwrap();
        assert_eq!(chain.len(), 3);
        // root, dep1, dep2 in traversal order
        let names: Vec<&str> = chain.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"root"));
        assert!(names.contains(&"dep1"));
        assert!(names.contains(&"dep2"));
    }

    #[test]
    fn test_pathfinding() {
        let store = test_store();
        let a = store
            .add_node(NodeKind::Module, "start", HashMap::new())
            .unwrap();
        let b = store
            .add_node(NodeKind::Module, "mid", HashMap::new())
            .unwrap();
        let c = store
            .add_node(NodeKind::Module, "end", HashMap::new())
            .unwrap();
        store
            .add_edge(&a, &b, EdgeKind::DependsOn, HashMap::new())
            .unwrap();
        store
            .add_edge(&b, &c, EdgeKind::DependsOn, HashMap::new())
            .unwrap();

        let paths = store.find_path(&a, &c).unwrap();
        assert!(!paths.is_empty());
    }
}
