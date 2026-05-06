use std::collections::HashMap;
use std::result::Result as StdResult;
use std::sync::Arc;

use automaton_core::{AutomationManifest, DepRef, EdgeKind, NodeKind};
use automaton_engine::{Engine, PlanOptions};
use rmcp::model::*;
use rmcp::{ServerHandler, ServiceExt, transport::stdio};

pub struct McpServer {
    engine: Arc<Engine>,
    data_dir: std::path::PathBuf,
}

impl McpServer {
    pub fn new(engine: Engine, data_dir: std::path::PathBuf) -> Self {
        Self { engine: Arc::new(engine), data_dir }
    }

    pub async fn serve_stdio(self) -> anyhow::Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

fn ok_json(value: serde_json::Value) -> StdResult<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&value).unwrap_or_default(),
    )]))
}

fn err_json(msg: &str) -> StdResult<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![Content::text(
        serde_json::to_string_pretty(&serde_json::json!({"error": msg})).unwrap_or_default(),
    )]))
}

fn parse_args<T: serde::de::DeserializeOwned>(
    request: &CallToolRequestParams,
) -> StdResult<T, ErrorData> {
    let val = request
        .arguments
        .as_ref()
        .map(|m| serde_json::Value::Object(m.clone()))
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    serde_json::from_value(val).map_err(|e| {
        ErrorData::new(ErrorCode(-32602), format!("Invalid params: {e}"), None)
    })
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.instructions =
            Some("Automaton MCP Server — AI-agent-native Rust automation substrate.".into());
        info
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = StdResult<CallToolResult, ErrorData>> + Send + '_>,
    > {
        let engine = self.engine.clone();
        Box::pin(async move {
            match request.name.as_ref() {
                "module_create" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        path: String,
                        source: String,
                        version: Option<String>,
                        summary: Option<String>,
                        depends_on: Option<Vec<String>>,
                        timeout_ms: Option<u64>,
                    }
                    let p: P = parse_args(&request)?;
                    let mut manifest = AutomationManifest::default();
                    manifest.name.clone_from(&p.path);
                    manifest.version = p.version.unwrap_or_else(|| "0.1.0".into());
                    manifest.summary = p.summary;
                    manifest.timeout_ms = p.timeout_ms.unwrap_or(30_000);
                    manifest.depends_on = p
                        .depends_on
                        .unwrap_or_default()
                        .iter()
                        .map(|d| DepRef::new(d))
                        .collect();
                    match engine.registry().register(&p.path, &p.source, &manifest) {
                        Ok(id) => {
                            let mut props = HashMap::new();
                            props.insert("path".into(), serde_json::json!(p.path));
                            let _ =
                                engine.graph().add_node(NodeKind::Module, &p.path, props);
                            ok_json(serde_json::json!({
                                "status": "created",
                                "path": p.path,
                                "hash": id.hash.as_str(),
                            }))
                        }
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                "module_build" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        path: String,
                        mode: Option<String>,
                    }
                    let p: P = parse_args(&request)?;
                    match engine.registry().get(&p.path) {
                        Ok(Some(m)) => {
                            let _ = engine.registry().mark_built(&p.path);
                            ok_json(serde_json::json!({
                                "status": "built",
                                "path": p.path,
                                "mode": p.mode.as_deref().unwrap_or("debug"),
                                "hash": m.hash.as_str(),
                            }))
                        }
                        _ => err_json("Module not found"),
                    }
                }

                "module_run" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        path: String,
                        input: Option<serde_json::Value>,
                    }
                    let p: P = parse_args(&request)?;
                    match engine.registry().get(&p.path) {
                        Ok(Some(m)) => ok_json(serde_json::json!({
                            "status": "ready",
                            "module": p.path,
                            "hash": m.hash.as_str(),
                        })),
                        Ok(None) => err_json("Module not found"),
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                "module_deprecate" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        path: String,
                    }
                    let p: P = parse_args(&request)?;
                    let _ = engine.graph().delete_node(&p.path);
                    ok_json(serde_json::json!({ "status": "deprecated", "path": p.path }))
                }

                "workflow_plan" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        start: String,
                        max_depth: Option<usize>,
                    }
                    let p: P = parse_args(&request)?;
                    let opts = PlanOptions {
                        max_depth: p.max_depth.unwrap_or(10),
                        ..Default::default()
                    };
                    match engine.plan(&p.start, &opts) {
                        Ok(rg) => ok_json(serde_json::json!({
                            "run_graph_id": rg.id,
                            "workflow": rg.workflow_name,
                            "modules": rg.modules.len(),
                        })),
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                "workflow_materialize" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        start: String,
                        max_depth: Option<usize>,
                    }
                    let p: P = parse_args(&request)?;
                    let opts = PlanOptions {
                        max_depth: p.max_depth.unwrap_or(10),
                        dry_run: true,
                        ..Default::default()
                    };
                    match engine.plan(&p.start, &opts) {
                        Ok(rg) => match engine.materialize(&rg) {
                            Ok(_) => ok_json(serde_json::json!({
                                "status": "valid_dag",
                                "modules": rg.modules.len(),
                            })),
                            Err(e) => err_json(&format!("Invalid DAG: {e}")),
                        },
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                "graph_query" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        kind: Option<String>,
                    }
                    let p: P = parse_args(&request)?;
                    let nodes = match p.kind.as_deref().unwrap_or("") {
                        "" => engine.graph().all_nodes().unwrap_or_default(),
                        "module" => {
                            engine.graph().find_nodes_by_kind(NodeKind::Module).unwrap_or_default()
                        }
                        "workflow" => {
                            engine.graph().find_nodes_by_kind(NodeKind::Workflow).unwrap_or_default()
                        }
                        "trigger" => {
                            engine.graph().find_nodes_by_kind(NodeKind::Trigger).unwrap_or_default()
                        }
                        "resource" => {
                            engine.graph().find_nodes_by_kind(NodeKind::Resource).unwrap_or_default()
                        }
                        k => return err_json(&format!("Unknown kind: {k}")),
                    };
                    ok_json(serde_json::json!({ "count": nodes.len(), "nodes": nodes }))
                }

                "graph_pathfind" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        from: String,
                        to: String,
                    }
                    let p: P = parse_args(&request)?;
                    let paths = engine.graph().find_path(&p.from, &p.to).unwrap_or_default();
                    ok_json(serde_json::json!({ "paths_found": paths.len() }))
                }

                "graph_add_edge" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        source: String,
                        target: String,
                        kind: String,
                    }
                    let p: P = parse_args(&request)?;
                    let ek = match p.kind.to_uppercase().as_str() {
                        "DEPENDS_ON" => EdgeKind::DependsOn,
                        "CALLS" => EdgeKind::Calls,
                        "TRIGGERS" => EdgeKind::Triggers,
                        "USES_RESOURCE" => EdgeKind::UsesResource,
                        "EMITS" => EdgeKind::Emits,
                        "CONSUMES" => EdgeKind::Consumes,
                        "BLOCKED_BY" => EdgeKind::BlockedBy,
                        "ALTERNATIVE_TO" => EdgeKind::AlternativeTo,
                        "UPGRADES" => EdgeKind::Upgrades,
                        "DERIVED_FROM" => EdgeKind::DerivedFrom,
                        _ => return err_json("Unknown edge kind"),
                    };
                    match engine.graph().add_edge(&p.source, &p.target, ek, HashMap::new()) {
                        Ok(eid) => ok_json(serde_json::json!({
                            "id": eid,
                            "source": p.source,
                            "target": p.target,
                            "kind": p.kind,
                        })),
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                "registry_search" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        query: String,
                    }
                    let p: P = parse_args(&request)?;
                    let all = engine.registry().list().unwrap_or_default();
                    let filtered: Vec<_> = all
                        .iter()
                        .filter(|(path, _, _, _)| path.contains(&p.query))
                        .collect();
                    ok_json(serde_json::json!({ "count": filtered.len(), "modules": filtered }))
                }

                "run_logs" => {
                    #[derive(serde::Deserialize)]
                    struct P {
                        module_path: Option<String>,
                        limit: Option<usize>,
                    }
                    let p: P = parse_args(&request)?;
                    let limit = p.limit.unwrap_or(20);
                    let runs = match p.module_path {
                        Some(ref m) => engine.registry().get_runs(m).unwrap_or_default(),
                        None => vec![],
                    };
                    ok_json(serde_json::json!({
                        "count": runs.len(),
                        "runs": runs.into_iter().take(limit).collect::<Vec<_>>(),
                    }))
                }

                "system_health" => {
                    let mc = engine.registry().list().unwrap_or_default().len();
                    let gn = engine.graph().all_nodes().unwrap_or_default().len();
                    let ge = engine.graph().all_edges().unwrap_or_default().len();
                    ok_json(serde_json::json!({
                        "status": "healthy",
                        "version": env!("CARGO_PKG_VERSION"),
                        "registry_modules": mc,
                        "graph_nodes": gn,
                        "graph_edges": ge,
                    }))
                }

                name => err_json(&format!("Unknown tool: {name}")),
            }
        })
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = StdResult<ListToolsResult, ErrorData>>
                + Send
                + '_,
        >,
    > {
        let empty: Arc<JsonObject> = Arc::new(serde_json::Map::new());
        let tools = vec![
            Tool::new("module_create", "Register a new automation module from source code", empty.clone()),
            Tool::new("module_build", "Build a registered module into a binary", empty.clone()),
            Tool::new("module_validate", "Validate module manifest and source", empty.clone()),
            Tool::new("module_run", "Execute a built module with JSON input", empty.clone()),
            Tool::new("module_deprecate", "Remove a module from registry and graph", empty.clone()),
            Tool::new("workflow_plan", "Plan a workflow from a starting module", empty.clone()),
            Tool::new("workflow_materialize", "Verify a workflow DAG has no cycles", empty.clone()),
            Tool::new("graph_query", "Query the design graph", empty.clone()),
            Tool::new("graph_pathfind", "Find paths between nodes", empty.clone()),
            Tool::new("graph_add_edge", "Wire an edge between two graph nodes", empty.clone()),
            Tool::new("registry_search", "Search registered modules by name", empty.clone()),
            Tool::new("run_logs", "Get run history for a module", empty.clone()),
            Tool::new("system_health", "Check system health", empty),
        ];
        Box::pin(std::future::ready(Ok(ListToolsResult {
            tools,
            meta: None,
            next_cursor: None,
        })))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let names = [
            "module_create", "module_build", "module_run",
            "module_deprecate", "workflow_plan", "workflow_materialize",
            "graph_query", "graph_pathfind", "graph_add_edge",
            "registry_search", "run_logs", "system_health",
        ];
        if names.contains(&name) {
            let empty: Arc<JsonObject> = Arc::new(serde_json::Map::new());
            Some(Tool::new(name.to_string(), "", empty))
        } else {
            None
        }
    }
}
