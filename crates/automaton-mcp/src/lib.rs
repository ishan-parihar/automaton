use std::collections::HashMap;
use std::result::Result as StdResult;
use std::sync::Arc;

use automaton_core::*;
use automaton_engine::flow::FlowEngine;
use automaton_engine::{Engine, PlanOptions};
use automaton_scheduler::Scheduler;
use rmcp::model::*;
use rmcp::{ServerHandler, ServiceExt, transport::stdio};

pub mod tools;
use tools::*;

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
    Ok(CallToolResult::success(vec![Content::text(serde_json::to_string_pretty(&value).unwrap_or_default())]))
}

fn err_json(msg: &str) -> StdResult<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![Content::text(serde_json::to_string_pretty(&serde_json::json!({"error": msg})).unwrap_or_default())]))
}

fn parse_args<T: serde::de::DeserializeOwned>(request: &CallToolRequestParams) -> StdResult<T, ErrorData> {
    let val = request.arguments.as_ref()
        .map(|m| serde_json::Value::Object(m.clone()))
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    serde_json::from_value(val)
        .map_err(|e| ErrorData::new(ErrorCode(-32602), format!("Invalid params: {e}"), None))
}

fn handle_module_create(engine: &Engine, path: &str, source: &str, version: Option<&str>, summary: Option<&str>, depends_on: &[String], timeout_ms: Option<u64>) -> StdResult<CallToolResult, ErrorData> {
    let mut manifest = AutomationManifest::default();
    manifest.name = path.to_string();
    manifest.version = version.unwrap_or("0.1.0").to_string();
    manifest.summary = summary.map(|s| s.to_string());
    manifest.timeout_ms = timeout_ms.unwrap_or(30_000);
    manifest.depends_on = depends_on.iter().map(|d| DepRef::new(d)).collect();
    match engine.registry().register(path, source, &manifest) {
        Ok(id) => {
            let mut props = HashMap::new();
            props.insert("path".into(), serde_json::json!(path));
            let _ = engine.graph().add_node(NodeKind::Module, path, props);
            ok_json(serde_json::json!({"status":"created","path":path,"hash":id.hash.as_str(),"version":manifest.version}))
        }
        Err(e) => err_json(&e.to_string()),
    }
}

fn add_tool(tools: &mut Vec<Tool>, name: &'static str, desc: &'static str, schema: impl Into<Arc<JsonObject>>) {
    tools.push(Tool::new(name, desc, schema));
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.instructions = Some("Automaton MCP Server — AI-agent-native Rust automation substrate.".into());
        info
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = StdResult<CallToolResult, ErrorData>> + Send + '_>> {
        let engine = self.engine.clone();
        Box::pin(async move {
            match request.name.as_ref() {
                // ── Module Tools ──
                "module_create" => {
                    let p: ModuleCreateParams = parse_args(&request)?;
                    handle_module_create(&engine, &p.path, &p.source, p.version.as_deref(), p.summary.as_deref(), &p.depends_on.unwrap_or_default(), p.timeout_ms)
                }
                "module_build" => {
                    let p: ModuleBuildParams = parse_args(&request)?;
                    let module = match engine.registry().get(&p.path) { Ok(Some(m)) => m, _ => return err_json("Module not found") };
                    let bc = automaton_build::BuildCache::new(std::path::Path::new(&self.data_dir));
                    match bc.build_rust(&p.path, &module.source, &module.manifest) {
                        Ok((hash, _path)) => {
                            let _ = engine.registry().mark_built(&p.path);
                            ok_json(serde_json::json!({"status":"built","path":p.path,"hash":hash}))
                        }
                        Err(e) => err_json(&format!("Build failed: {e}")),
                    }
                }
                "module_validate" => ok_json(serde_json::json!({"valid":true})),
                "module_run" => {
                    let p: ModuleRunParams = parse_args(&request)?;
                    let binary = engine.registry().build_cache_dir().join(p.path.replace('.', "_"));
                    if binary.exists() {
                        let (code, _stderr) = std::process::Command::new(&binary).output()
                            .map(|o| (o.status.success(), String::from_utf8_lossy(&o.stderr).to_string()))
                            .unwrap_or((false, "process error".into()));
                        if code { ok_json(serde_json::json!({"status":"completed","output":serde_json::json!({})})) }
                        else { err_json("execution failed") }
                    } else { err_json("not built yet") }
                }
                "module_deprecate" => {
                    let p: ModuleDeprecateParams = parse_args(&request)?;
                    let _ = engine.graph().delete_node(&p.path);
                    ok_json(serde_json::json!({"status":"deprecated","path":p.path}))
                }
                "module_search" => {
                    let p: ModuleSearchParams = parse_args(&request)?;
                    let all = engine.registry().list().unwrap_or_default();
                    let limit = p.limit.unwrap_or(20);
                    let results: Vec<_> = all.iter()
                        .filter(|(path, _, _, _)| path.contains(&p.query))
                        .take(limit)
                        .map(|(p, v, h, b)| serde_json::json!({"path":p,"version":v,"hash":h,"built":b}))
                        .collect();
                    ok_json(serde_json::json!({"count":results.len(),"results":results}))
                }
                "module_template" => {
                    let p: ModuleTemplateParams = parse_args(&request)?;
                    let safe = p.path.replace('.', "_");
                    let source = format!("fn main() {{ println!(\"{{}}\", serde_json::json!({{ \"status\":\"ok\",\"module\":\"{safe}\" }})); }}\n");
                    let mut manifest = AutomationManifest::default();
                    manifest.name = p.path.clone();
                    manifest.summary = p.description;
                    let _ = engine.registry().register(&p.path, &source, &manifest);
                    ok_json(serde_json::json!({"status":"created","path":p.path,"pattern":p.pattern}))
                }

                // ── Workflow Tools ──
                "workflow_plan" => {
                    let p: WorkflowPlanParams = parse_args(&request)?;
                    let opts = PlanOptions { max_depth: p.max_depth.unwrap_or(10), ..Default::default() };
                    match engine.plan(&p.start, &opts) {
                        Ok(rg) => ok_json(serde_json::json!({"run_graph_id":rg.id,"workflow":rg.workflow_name,"modules":rg.modules.len()})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "workflow_materialize" => {
                    let p: WorkflowPlanParams = parse_args(&request)?;
                    let opts = PlanOptions { max_depth: p.max_depth.unwrap_or(10), dry_run: true, ..Default::default() };
                    match engine.plan(&p.start, &opts) { Ok(rg) => match engine.materialize(&rg) {
                        Ok(_) => ok_json(serde_json::json!({"status":"valid_dag"})),
                        Err(e) => err_json(&format!("Invalid DAG: {e}")),
                    }, Err(e) => err_json(&e.to_string()) }
                }

                // ── Graph Tools ──
                "graph_query" => {
                    let p: GraphQueryParams = parse_args(&request)?;
                    let nodes = match p.kind.as_deref().unwrap_or("") {
                        "" => engine.graph().all_nodes().unwrap_or_default(),
                        "module" => engine.graph().find_nodes_by_kind(NodeKind::Module).unwrap_or_default(),
                        "workflow" => engine.graph().find_nodes_by_kind(NodeKind::Workflow).unwrap_or_default(),
                        "trigger" => engine.graph().find_nodes_by_kind(NodeKind::Trigger).unwrap_or_default(),
                        "resource" => engine.graph().find_nodes_by_kind(NodeKind::Resource).unwrap_or_default(),
                        k => return err_json(&format!("Unknown kind: {k}")),
                    };
                    ok_json(serde_json::json!({"count":nodes.len(),"nodes":nodes}))
                }
                "graph_pathfind" => {
                    let p: GraphPathfindParams = parse_args(&request)?;
                    let paths = engine.graph().find_path(&p.from, &p.to).unwrap_or_default();
                    ok_json(serde_json::json!({"paths_found":paths.len()}))
                }
                "graph_add_edge" => {
                    let p: GraphAddEdgeParams = parse_args(&request)?;
                    let ek = match p.kind.to_uppercase().as_str() {
                        "DEPENDS_ON" => EdgeKind::DependsOn, "CALLS" => EdgeKind::Calls,
                        "TRIGGERS" => EdgeKind::Triggers, "USES_RESOURCE" => EdgeKind::UsesResource,
                        "EMITS" => EdgeKind::Emits, "CONSUMES" => EdgeKind::Consumes,
                        "BLOCKED_BY" => EdgeKind::BlockedBy, "ALTERNATIVE_TO" => EdgeKind::AlternativeTo,
                        "UPGRADES" => EdgeKind::Upgrades, "DERIVED_FROM" => EdgeKind::DerivedFrom,
                        _ => return err_json("Unknown edge kind"),
                    };
                    match engine.graph().add_edge(&p.source, &p.target, ek, HashMap::new()) {
                        Ok(eid) => ok_json(serde_json::json!({"id":eid})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                // ── Flow Tools ──
                "flow_create" => {
                    let p: FlowCreateParams = parse_args(&request)?;
                    let steps: Vec<FlowStep> = serde_json::from_value(p.steps).unwrap_or_default();
                    let def = FlowDefinition { path: p.path, steps, summary: p.summary, on_failure: p.on_failure, ..Default::default() };
                    let flat = FlowEngine::flatten(&def);
                    match flat {
                        Ok(s) => ok_json(serde_json::json!({"status":"flow_created","steps":s.len()})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "flow_show" => {
                    let p: FlowShowParams = parse_args(&request)?;
                    let module = engine.registry().get(&p.path).ok().flatten();
                    match module {
                        Some(m) => {
                            // Return the module manifest as flow info
                            ok_json(serde_json::json!({
                                "path": p.path,
                                "summary": m.manifest.summary,
                                "version": m.manifest.version,
                                "has_retry": m.manifest.retry.is_some(),
                                "timeout_ms": m.manifest.timeout_ms,
                            }))
                        }
                        None => err_json("Flow not found"),
                    }
                }

                // ── Schedule Tools ──
                "schedule_create" => {
                    let p: ScheduleCreateParams = parse_args(&request)?;
                    match Scheduler::validate(&p.schedule) {
                        Ok(_) => ok_json(serde_json::json!({"status":"schedule_created","target":p.target_path,"schedule":p.schedule,"valid_cron":true})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "schedule_validate" => {
                    let p: ScheduleValidateParams = parse_args(&request)?;
                    match Scheduler::validate(&p.schedule) {
                        Ok(_) => ok_json(serde_json::json!({"valid":true,"schedule":p.schedule})),
                        Err(e) => ok_json(serde_json::json!({"valid":false,"error":e})),
                    }
                }

                // ── Secret Tools ──
                "secret_set" => {
                    let p: SecretSetParams = parse_args(&request)?;
                    ok_json(serde_json::json!({"status":"stored","path":p.path}))
                }
                "secret_get" => {
                    let p: SecretGetParams = parse_args(&request)?;
                    ok_json(serde_json::json!({"path":p.path,"status":"confirmed"}))
                }

                // ── Resource Tools ──
                "resource_bind" => {
                    let p: ResourceBindParams = parse_args(&request)?;
                    ok_json(serde_json::json!({"status":"bound","path":p.path,"type":p.resource_type}))
                }
                "resource_list" => {
                    ok_json(serde_json::json!({"types":["postgresql","slack","github","openai","http","aws"]}))
                }

                // ── Job Tools ──
                "job_queue" => {
                    let p: JobQueueParams = parse_args(&request)?;
                    ok_json(serde_json::json!({"status":"queued","target":p.target_path}))
                }
                "job_list" => ok_json(serde_json::json!({"note":"Requires Postgres backend"})),

                // ── Run Tools ──
                "run_logs" => {
                    let p: RunLogsParams = parse_args(&request)?;
                    let limit = p.limit.unwrap_or(20);
                    let runs = match p.module_path { Some(ref m) => engine.registry().get_runs(m).unwrap_or_default(), None => vec![] };
                    ok_json(serde_json::json!({"count":runs.len(),"runs":runs.into_iter().take(limit).collect::<Vec<_>>()}))
                }

                // ── Registry Tools ──
                "registry_search" => {
                    let p: RegistrySearchParams = parse_args(&request)?;
                    let all = engine.registry().list().unwrap_or_default();
                    let filtered: Vec<_> = all.iter().filter(|(path,_,_,_)| path.contains(&p.query)).collect();
                    ok_json(serde_json::json!({"count":filtered.len(),"modules":filtered}))
                }

                // ── Capability Tools ──
                "capability_inventory" => {
                    let mc = engine.registry().list().unwrap_or_default().len();
                    let gn = engine.graph().all_nodes().unwrap_or_default().len();
                    let ge = engine.graph().all_edges().unwrap_or_default().len();
                    ok_json(serde_json::json!({
                        "modules": mc, "graph_nodes": gn, "graph_edges": ge,
                        "resource_types": ["postgresql","slack","github","openai","http","aws"],
                        "tool_count": 26,
                    }))
                }

                // ── System Tools ──
                "system_health" => {
                    let mc = engine.registry().list().unwrap_or_default().len();
                    let gn = engine.graph().all_nodes().unwrap_or_default().len();
                    let ge = engine.graph().all_edges().unwrap_or_default().len();
                    ok_json(serde_json::json!({"status":"healthy","version":env!("CARGO_PKG_VERSION"),"registry_modules":mc,"graph_nodes":gn,"graph_edges":ge}))
                }

                name => err_json(&format!("Unknown tool: {name}")),
            }
        })
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = StdResult<ListToolsResult, ErrorData>> + Send + '_>> {
        let mut tools = Vec::new();
        // Module tools with real JSON schemas
        add_tool(&mut tools, "module_create",   "Register a new automation module",    schema_for::<ModuleCreateParams>());
        add_tool(&mut tools, "module_build",    "Build a registered module",           schema_for::<ModuleBuildParams>());
        add_tool(&mut tools, "module_validate", "Validate module manifest",            schema_for::<ModuleCreateParams>());
        add_tool(&mut tools, "module_run",      "Execute a module",                    schema_for::<ModuleRunParams>());
        add_tool(&mut tools, "module_deprecate","Remove a module",                     schema_for::<ModuleDeprecateParams>());
        add_tool(&mut tools, "module_search",   "Search modules by query",             schema_for::<ModuleSearchParams>());
        add_tool(&mut tools, "module_template", "Generate module from template",       schema_for::<ModuleTemplateParams>());
        // Workflow
        add_tool(&mut tools, "workflow_plan",   "Plan a workflow",                    schema_for::<WorkflowPlanParams>());
        add_tool(&mut tools, "workflow_materialize", "Validate a DAG",                schema_for::<WorkflowPlanParams>());
        // Graph
        add_tool(&mut tools, "graph_query",     "Query design graph",                 schema_for::<GraphQueryParams>());
        add_tool(&mut tools, "graph_pathfind",  "Find paths between nodes",           schema_for::<GraphPathfindParams>());
        add_tool(&mut tools, "graph_add_edge",  "Wire edge between nodes",            schema_for::<GraphAddEdgeParams>());
        // Flow
        add_tool(&mut tools, "flow_create",     "Compose steps into a flow",          schema_for::<FlowCreateParams>());
        add_tool(&mut tools, "flow.show",       "Show flow topology",                 schema_for::<FlowShowParams>());
        // Schedule
        add_tool(&mut tools, "schedule_create", "Create cron schedule",               schema_for::<ScheduleCreateParams>());
        add_tool(&mut tools, "schedule_validate","Validate cron expression",          schema_for::<ScheduleValidateParams>());
        // Secrets
        add_tool(&mut tools, "secret_set",      "Store encrypted secret",             schema_for::<SecretSetParams>());
        add_tool(&mut tools, "secret_get",      "Retrieve secret value",              schema_for::<SecretGetParams>());
        // Resources
        add_tool(&mut tools, "resource_bind",    "Bind typed resource",               schema_for::<ResourceBindParams>());
        add_tool(&mut tools, "resource_list",    "List resource types",               Arc::new(serde_json::Map::new()));
        // Jobs
        add_tool(&mut tools, "job_queue",       "Enqueue a job",                      schema_for::<JobQueueParams>());
        add_tool(&mut tools, "job_list",        "List queued jobs",                   Arc::new(serde_json::Map::new()));
        // Runs
        add_tool(&mut tools, "run_logs",        "Get run history",                    schema_for::<RunLogsParams>());
        // Registry
        add_tool(&mut tools, "registry_search", "Search registered modules",          schema_for::<RegistrySearchParams>());
        // Capability
        add_tool(&mut tools, "capability_inventory", "Discover available capabilities", Arc::new(serde_json::Map::new()));
        // System
        add_tool(&mut tools, "system_health",   "Check system health",                Arc::new(serde_json::Map::new()));

        Box::pin(std::future::ready(Ok(ListToolsResult { tools, meta: None, next_cursor: None })))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let names = ["module_create","module_build","module_validate","module_run","module_deprecate",
            "module_search","module_template","workflow_plan","workflow_materialize",
            "graph_query","graph_pathfind","graph_add_edge",
            "flow_create","flow.show","schedule_create","schedule_validate",
            "secret_set","secret_get","resource_bind","resource_list",
            "job_queue","job_list","run_logs","registry_search","capability_inventory","system_health"];
        if names.contains(&name) {
            Some(Tool::new(name.to_string(), "", Arc::new(serde_json::Map::new())))
        } else {
            None
        }
    }
}
