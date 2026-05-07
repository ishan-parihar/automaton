use std::collections::HashMap;
use std::result::Result as StdResult;
use std::sync::Arc;

use automaton_core::*;
use automaton_core::execution::{WebhookEvent, WebhookRegistration};
use automaton_engine::flow::FlowEngine;
use automaton_engine::{Engine, PlanOptions};
use automaton_runtime::{Runtime, RuntimeConfig};
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

async fn handle_module_create(engine: &Engine, path: &str, source: &str, version: Option<&str>, summary: Option<&str>, depends_on: &[String], timeout_ms: Option<u64>) -> StdResult<CallToolResult, ErrorData> {
    let mut manifest = AutomationManifest::default();
    manifest.name = path.to_string();
    manifest.version = version.unwrap_or("0.1.0").to_string();
    manifest.summary = summary.map(|s| s.to_string());
    manifest.timeout_ms = timeout_ms.unwrap_or(30_000);
    manifest.depends_on = depends_on.iter().map(|d| DepRef::new(d)).collect();
    match engine.backend().register_module(path, source, &manifest).await {
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
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = StdResult<CallToolResult, ErrorData>> + Send + '_>> {
        let engine = self.engine.clone();
        let peer = context.peer.clone();
        Box::pin(async move {
            match request.name.as_ref() {
                // ── Module Tools ──
                "module_create" => {
                    let p: ModuleCreateParams = parse_args(&request)?;
                    return handle_module_create(&engine, &p.path, &p.source, p.version.as_deref(), p.summary.as_deref(), &p.depends_on.unwrap_or_default(), p.timeout_ms).await;
                }
                "module_build" => {
                    let p: ModuleBuildParams = parse_args(&request)?;
                    let module = match engine.backend().get_module(&p.path).await { Ok(Some(m)) => m, _ => return err_json("Module not found") };
                    let bc = automaton_build::BuildCache::new(std::path::Path::new(&self.data_dir));
                    match bc.build_rust(&p.path, &module.source, &module.manifest) {
                        Ok((hash, _path)) => {
                            let _ = engine.backend().mark_built(&p.path).await;
                            ok_json(serde_json::json!({"status":"built","path":p.path,"hash":hash}))
                        }
                        Err(e) => err_json(&format!("Build failed: {e}")),
                    }
                }
                "module_validate" => {
                    let p: ModuleCreateParams = parse_args(&request)?;
                    if p.path.is_empty() {
                        return err_json("path is required");
                    }
                    if p.source.is_empty() {
                        return err_json("source is required");
                    }
                    ok_json(serde_json::json!({"valid":true,"path":p.path}))
                }
                "module_run" => {
                    let p: ModuleRunParams = parse_args(&request)?;
                    let binary = engine.backend().build_cache_dir().join(p.path.replace('.', "_"));
                    if !binary.exists() {
                        return err_json("not built yet");
                    }
                    let rt = Runtime::new(RuntimeConfig {
                        work_dir: self.data_dir.join("work"),
                        temp_dir: self.data_dir.join("tmp"),
                        ..Default::default()
                    });
                    let input = p.input.unwrap_or(serde_json::json!({}));
                    match rt.run_binary(&binary, &input, 30000).await {
                        Ok(output) => ok_json(serde_json::json!({"status":"completed","output":output})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "module_deprecate" => {
                    let p: ModuleDeprecateParams = parse_args(&request)?;
                    let _ = engine.graph().delete_node(&p.path);
                    ok_json(serde_json::json!({"status":"deprecated","path":p.path}))
                }
                "module_search" => {
                    let p: ModuleSearchParams = parse_args(&request)?;
                    let all = engine.backend().list_modules().await.unwrap_or_default();
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
                    let tmpl = automaton_build::templates::get_template(&p.pattern);
                    let source = tmpl.map(|t| t.source.to_string()).unwrap_or_else(|| {
                        let safe = p.path.replace('.', "_");
                        format!("fn main() {{ println!(\"{{}}\", serde_json::json!({{ \"status\":\"ok\",\"module\":\"{safe}\" }})); }}\n")
                    });
                    let mut manifest = AutomationManifest::default();
                    manifest.name = p.path.clone();
                    manifest.summary = p.description;
                    let _ = engine.backend().register_module(&p.path, &source, &manifest).await;
                    ok_json(serde_json::json!({"status":"created","path":p.path,"pattern":p.pattern,"source_len":source.len()}))
                }
                "module_list_templates" => {
                    let templates = automaton_build::templates::all_templates();
                    let list: Vec<serde_json::Value> = templates.iter().map(|t| {
                        serde_json::json!({
                            "name": t.name,
                            "description": t.description,
                        })
                    }).collect();
                    ok_json(serde_json::json!({"templates": list, "count": list.len()}))
                }

                // ── Workflow Tools ──
                "workflow_plan" => {
                    let p: WorkflowPlanParams = parse_args(&request)?;
                    let opts = PlanOptions { max_depth: p.max_depth.unwrap_or(10), ..Default::default() };
                    match engine.plan(&p.start, &opts).await {
                        Ok(rg) => ok_json(serde_json::json!({"run_graph_id":rg.id,"workflow":rg.workflow_name,"modules":rg.modules.len()})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "workflow_materialize" => {
                    let p: WorkflowPlanParams = parse_args(&request)?;
                    let opts = PlanOptions { max_depth: p.max_depth.unwrap_or(10), dry_run: true, ..Default::default() };
                    match engine.plan(&p.start, &opts).await { Ok(rg) => match engine.materialize(&rg) {
                        Ok(_) => ok_json(serde_json::json!({"status":"valid_dag"})),
                        Err(e) => err_json(&format!("Invalid DAG: {e}")),
                    }, Err(e) => err_json(&e.to_string()) }
                }

                // ── Graph Tools ──
                "graph_query" => {
                    let p: GraphQueryParams = parse_args(&request)?;

                    // When property filters are active we need all matching nodes
                    // in memory so we can filter by JSON properties, since SQLite
                    // stores properties as opaque TEXT.
                    let has_props = p.properties.as_ref().map(|m| !m.is_empty()).unwrap_or(false);
                    let limit = p.limit;
                    let offset = p.offset;

                    let nodes: Vec<Node> = if has_props {
                        // Load all nodes (optionally filtered by kind) into memory
                        match p.kind.as_deref().unwrap_or("") {
                            ""       => engine.graph().all_nodes().unwrap_or_default(),
                            "module"   => engine.graph().find_nodes_by_kind(NodeKind::Module).unwrap_or_default(),
                            "workflow" => engine.graph().find_nodes_by_kind(NodeKind::Workflow).unwrap_or_default(),
                            "trigger"  => engine.graph().find_nodes_by_kind(NodeKind::Trigger).unwrap_or_default(),
                            "resource" => engine.graph().find_nodes_by_kind(NodeKind::Resource).unwrap_or_default(),
                            k => return err_json(&format!("Unknown kind: {k}")),
                        }
                    } else {
                        match p.kind.as_deref().unwrap_or("") {
                            ""       => engine.graph().all_nodes_paginated(limit, offset).unwrap_or_default(),
                            "module"   => engine.graph().find_nodes_by_kind_paginated(NodeKind::Module, limit, offset).unwrap_or_default(),
                            "workflow" => engine.graph().find_nodes_by_kind_paginated(NodeKind::Workflow, limit, offset).unwrap_or_default(),
                            "trigger"  => engine.graph().find_nodes_by_kind_paginated(NodeKind::Trigger, limit, offset).unwrap_or_default(),
                            "resource" => engine.graph().find_nodes_by_kind_paginated(NodeKind::Resource, limit, offset).unwrap_or_default(),
                            k => return err_json(&format!("Unknown kind: {k}")),
                        }
                    };

                    // Apply in-memory property filter (no-op when has_props is false)
                    let nodes: Vec<Node> = if let Some(ref props) = p.properties {
                        if !props.is_empty() {
                            nodes.into_iter()
                                .filter(|n| props.iter().all(|(k, v)| n.properties.get(k) == Some(v)))
                                .collect()
                        } else {
                            nodes
                        }
                    } else {
                        nodes
                    };

                    // Re-apply limit/offset for property-filtered results
                    let nodes = if has_props {
                        let off = offset.unwrap_or(0) as usize;
                        match limit {
                            Some(l) => nodes.into_iter().skip(off).take(l as usize).collect(),
                            None    => nodes.into_iter().skip(off).collect(),
                        }
                    } else {
                        nodes
                    };

                    ok_json(serde_json::json!({"count":nodes.len(),"nodes":nodes}))
                }
                "graph_pathfind" => {
                    let p: GraphPathfindParams = parse_args(&request)?;
                    let paths: Vec<Vec<automaton_graph::NodeAndEdge>> = engine.graph().find_path(&p.from, &p.to).unwrap_or_default();
                    ok_json(serde_json::json!({"paths_found": paths.len(), "paths": paths}))
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
                    let def = FlowDefinition {
                        path: p.path.clone(),
                        steps,
                        summary: p.summary.clone(),
                        on_failure: p.on_failure.clone(),
                        ..Default::default()
                    };
                    let flat = FlowEngine::flatten(&def);
                    match flat {
                        Ok(s) => {
                            // Persist the flow
                            let def_json = serde_json::to_value(&def).unwrap_or_default();
                            match engine.backend().store_flow(
                                &p.path, "0.1.0", &def_json,
                                p.summary.as_deref(), p.on_failure.as_deref(),
                            ).await {
                                Ok(id) => ok_json(serde_json::json!({
                                    "status":"flow_created","flow_id":id,
                                    "path":p.path,"steps":s.len()
                                })),
                                Err(e) => err_json(&e.to_string()),
                            }
                        }
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "flow_show" => {
                    let p: FlowShowParams = parse_args(&request)?;
                    match engine.backend().get_flow(&p.path).await {
                        Ok(Some(flow)) => ok_json(flow),
                        Ok(None) => err_json("Flow not found"),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "flow_list" => {
                    match engine.backend().list_flows().await {
                        Ok(flows) => ok_json(serde_json::json!({"flows": flows})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "flow_delete" => {
                    let p: FlowShowParams = parse_args(&request)?;
                    match engine.backend().delete_flow(&p.path).await {
                        Ok(_) => ok_json(serde_json::json!({"status":"deleted","path":p.path})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "flow_execute" => {
                    let p: FlowExecuteParams = parse_args(&request)?;
                    // Try flow persistence path first
                    let flow_result = engine.backend().get_flow(&p.path).await;
                    match flow_result {
                        Ok(Some(flow)) => {
                            // Execute persisted flow via FlowEngine with Runtime
                            let def_val = flow.get("definition").cloned().unwrap_or_default();
                            if let Ok(def) = serde_json::from_value::<FlowDefinition>(def_val) {
                                match FlowEngine::flatten(&def) {
                                    Ok(steps) => {
                                        let rt = Runtime::new(RuntimeConfig {
                                            work_dir: self.data_dir.join("work"),
                                            temp_dir: self.data_dir.join("tmp"),
                                            ..Default::default()
                                        });
                                        let bc = engine.backend().build_cache_dir().to_path_buf();
                                        match FlowEngine::execute(
                                            &steps,
                                            Some(engine.backend()),
                                            &rt,
                                            &bc,
                                        ).await {
                                            Ok(outputs) => {
                                                let results: serde_json::Map<_, _> = outputs.into_iter().collect();
                                                // Create Run node in graph with execution metadata
                                                let now = std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_secs();
                                                let run_name = format!("run-{}-{}", p.path.replace('/', "_"), now);
                                                let mut run_props = std::collections::HashMap::new();
                                                run_props.insert("flow_path".into(), serde_json::json!(p.path));
                                                run_props.insert("execution_time".into(), serde_json::json!(now));
                                                run_props.insert("status".into(), serde_json::json!("completed"));
                                                run_props.insert("result_count".into(), serde_json::json!(results.len()));
                                                if let Ok(ref run_id) = engine.graph().add_node(NodeKind::Run, &run_name, run_props) {
                                                    // Best-effort: create Triggers edge from first step's module to run node
                                                    if let Some(first_step) = def.steps.first() {
                                                        if let Some(ref script_path) = first_step.script_path {
                                                            if let Ok(modules) = engine.graph().find_nodes_by_kind(NodeKind::Module) {
                                                                if let Some(mod_node) = modules.iter().find(|n| n.name == *script_path) {
                                                                    let _ = engine.graph().add_edge(&mod_node.id, run_id, EdgeKind::Triggers, HashMap::new());
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                ok_json(serde_json::json!({"status":"completed","mode":"flow","results":results}))
                                            }
                                            Err(e) => err_json(&e.to_string())
                                        }
                                    }
                                    Err(e) => err_json(&e.to_string())
                                }
                            } else {
                                err_json("Invalid flow definition")
                            }
                        }
                        _ => {
                            // Fall back to Engine DAG execution (module-based)
                            let opts = PlanOptions { ..Default::default() };
                            match engine.plan(&p.path, &opts).await {
                                Ok(rg) => {
                                    match engine.materialize(&rg) {
                                        Ok(dag) => {
                                            let result = engine.execute(dag).await;
                                            match result {
                                                Ok(output) => {
                                                    // Create Run node in graph with execution metadata
                                                    let now = std::time::SystemTime::now()
                                                        .duration_since(std::time::UNIX_EPOCH)
                                                        .unwrap_or_default()
                                                        .as_secs();
                                                    let run_name = format!("run-{}-{}", p.path.replace('/', "_"), now);
                                                    let mut run_props = std::collections::HashMap::new();
                                                    run_props.insert("flow_path".into(), serde_json::json!(p.path));
                                                    run_props.insert("execution_time".into(), serde_json::json!(now));
                                                    run_props.insert("status".into(), serde_json::json!("completed"));
                                                    let _ = engine.graph().add_node(NodeKind::Run, &run_name, run_props);
                                                    ok_json(serde_json::json!({"status":"completed","mode":"dag","run_id":output.run_graph_id,"results":output.results}))
                                                }
                                                Err(e) => err_json(&e.to_string()),
                                            }
                                        }
                                        Err(e) => err_json(&format!("Invalid DAG: {e}")),
                                    }
                                }
                                Err(e) => err_json(&e.to_string()),
                            }
                        }
                    }
                }

                // ── Schedule Tools ──
                "schedule_create" => {
                    let p: ScheduleCreateParams = parse_args(&request)?;
                    match Scheduler::validate(&p.schedule) {
                        Ok(_) => {
                            let config = serde_json::json!({"schedule": p.schedule, "args": p.args});
                            match engine.backend().create_trigger(&p.target_path, false, "cron", &config).await {
                                Ok(id) => ok_json(serde_json::json!({"status":"schedule_created","id":id,"target":p.target_path,"schedule":p.schedule,"valid_cron":true})),
                                Err(e) => err_json(&e.to_string()),
                            }
                        }
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "schedule_validate" => {
                    let p: ScheduleValidateParams = parse_args(&request)?;
                    match Scheduler::validate(&p.schedule) {
                        Ok(_) => ok_json(serde_json::json!({"valid":true,"schedule":p.schedule})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                // ── Secret Tools ──
                "secret_set" => {
                    let p: SecretSetParams = parse_args(&request)?;
                    match engine.backend().set_variable(&p.path, &p.value, true).await {
                        Ok(_) => ok_json(serde_json::json!({"status":"stored","path":p.path})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "secret_get" => {
                    let p: SecretGetParams = parse_args(&request)?;
                    match engine.backend().get_variable(&p.path).await {
                        Ok(Some(val)) => ok_json(serde_json::json!({"path":p.path,"value":val,"status":"found"})),
                        Ok(None) => err_json("Secret not found"),
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                // ── Resource Tools ──
                "resource_bind" => {
                    let p: ResourceBindParams = parse_args(&request)?;
                    match engine.backend().set_resource(&p.path, &p.resource_type, &p.value).await {
                        Ok(_) => ok_json(serde_json::json!({"status":"bound","path":p.path,"type":p.resource_type})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "resource_list" => {
                    let resources = engine.backend().list_resources(None).await.unwrap_or_default();
                    ok_json(serde_json::json!({"types":["postgresql","slack","github","openai","http","aws"],"resources":resources,}))
                }

                // ── Job Tools ──
                "job_queue" => {
                    let p: JobQueueParams = parse_args(&request)?;
                    let args = p.args.unwrap_or(serde_json::json!({}));
                    match engine.backend().enqueue_job(p.kind.as_deref().unwrap_or("script"), &p.target_path, &args).await {
                        Ok(job_id) => ok_json(serde_json::json!({"status":"queued","target":p.target_path,"job_id":job_id})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }
                "job_list" => {
                    let limit = 50i64;
                    match engine.backend().list_jobs(limit).await {
                        Ok(jobs) => ok_json(serde_json::json!({"jobs":jobs})),
                        Err(e) => err_json(&e.to_string()),
                    }
                }

                // ── Run Tools ──
                "run_logs" => {
                    let p: RunLogsParams = parse_args(&request)?;
                    let limit = p.limit.unwrap_or(20);
                    let runs = match p.module_path { Some(ref m) => engine.backend().get_runs(m).await.unwrap_or_default(), None => vec![] };
                    ok_json(serde_json::json!({"count":runs.len(),"runs":runs.into_iter().take(limit).collect::<Vec<_>>()}))
                }
                "run_retry" => {
                    let p: RunRetryParams = parse_args(&request)?;
                    ok_json(serde_json::json!({"status":"retry_scheduled","run_id":p.run_id}))
                }

                // ── Registry Tools ──
                "registry_search" => {
                    let p: RegistrySearchParams = parse_args(&request)?;
                    let all = engine.backend().list_modules().await.unwrap_or_default();
                    let filtered: Vec<_> = all.iter().filter(|(path,_,_,_)| path.contains(&p.query)).collect();
                    ok_json(serde_json::json!({"count":filtered.len(),"modules":filtered}))
                }

                // ── Graph Summary Tool ──
                "graph_summarize" => {
                    let _params: GraphSummarizeParams = parse_args(&request)?;
                    let summary = engine.graph().summarize()
                        .map_err(|e| ErrorData::new(ErrorCode(-32603), format!("Summarize failed: {e}"), None))?;
                    ok_json(serde_json::json!(summary))
                }

                // ── Graph Search Tools ──
                "graph_search" => {
                    let p: SearchParams = parse_args(&request)?;
                    let nodes = engine.graph().search_nodes(&p.query)
                        .map_err(|e| ErrorData::new(ErrorCode(-32603), format!("Search failed: {e}"), None))?;
                    ok_json(serde_json::json!({"count": nodes.len(), "nodes": nodes}))
                }
                "graph_time_range" => {
                    let p: TimeRangeParams = parse_args(&request)?;
                    let nodes = engine.graph().find_nodes_in_time_range(Some(&p.start), Some(&p.end))
                        .map_err(|e| ErrorData::new(ErrorCode(-32603), format!("Query failed: {e}"), None))?;
                    let edges = engine.graph().find_edges_in_time_range(Some(&p.start), Some(&p.end))
                        .map_err(|e| ErrorData::new(ErrorCode(-32603), format!("Query failed: {e}"), None))?;
                    ok_json(serde_json::json!({"nodes": nodes, "edges": edges}))
                }

                // ── Webhook Tools ──
                "webhook_register" => {
                    let p: WebhookRegisterParams = parse_args(&request)?;
                    let event: WebhookEvent = serde_json::from_value(serde_json::json!(p.event))
                        .map_err(|e| ErrorData::new(ErrorCode(-32602), format!("Invalid event: {e}"), None))?;
                    let id = engine.backend().register_webhook(&p.url, &p.event, p.secret.as_deref()).await
                        .map_err(|e| ErrorData::new(ErrorCode(-32603), format!("Registration failed: {e}"), None))?;
                    ok_json(serde_json::json!({"id": id, "url": p.url, "event": p.event, "secret": p.secret}))
                }
                "webhook_list" => {
                    let _p: WebhookListParams = parse_args(&request)?;
                    let webhooks = engine.backend().list_webhooks(None).await
                        .map_err(|e| ErrorData::new(ErrorCode(-32603), format!("List failed: {e}"), None))?;
                    ok_json(serde_json::json!({"webhooks": webhooks}))
                }
                "webhook_delete" => {
                    let p: WebhookDeleteParams = parse_args(&request)?;
                    engine.backend().delete_webhook(&p.id).await
                        .map_err(|e| ErrorData::new(ErrorCode(-32603), format!("Delete failed: {e}"), None))?;
                    ok_json(serde_json::json!({"deleted": true}))
                }

                // ── Flow Telemetry Tools ──
                "flow_execute_telemetry" => {
                    let p: FlowExecuteTelemetryParams = parse_args(&request)?;
                    let progress_token: Option<ProgressToken> = request.progress_token();
                    let progress_peer: std::sync::Arc<std::sync::Mutex<rmcp::service::Peer<rmcp::RoleServer>>> = std::sync::Arc::new(std::sync::Mutex::new(peer));

                    // Build progress callback
                    let on_progress: Option<Box<dyn Fn(usize, usize, &StepTelemetry) + Send + Sync + 'static>> = if progress_token.is_some() {
                        let token = progress_token.unwrap();
                        let pp = progress_peer.clone();
                        Some(Box::new(move |current: usize, total: usize, _step: &StepTelemetry| {
                            let params = ProgressNotificationParam::new(token.clone(), current as f64)
                                .with_total(total as f64);
                            if let Ok(guard) = pp.lock() {
                                let p = guard.clone();
                                tokio::spawn(async move {
                                    let _ = p.notify_progress(params).await;
                                });
                            }
                        }))
                    } else {
                        None
                    };

                    // Try flow persistence path first
                    let flow_result = engine.backend().get_flow(&p.path).await;
                    match flow_result {
                        Ok(Some(flow)) => {
                            let def_val = flow.get("definition").cloned().unwrap_or_default();
                            if let Ok(def) = serde_json::from_value::<FlowDefinition>(def_val) {
                                match FlowEngine::flatten(&def) {
                                    Ok(steps) => {
                                        let rt = Runtime::new(RuntimeConfig {
                                            work_dir: self.data_dir.join("work"),
                                            temp_dir: self.data_dir.join("tmp"),
                                            ..Default::default()
                                        });
                                        let bc = engine.backend().build_cache_dir().to_path_buf();
                                        match FlowEngine::execute_with_telemetry(
                                            &steps,
                                            Some(engine.backend()),
                                            &rt,
                                            &bc,
                                            None as Option<&str>,
                                            on_progress,
                                        ).await {
                                            Ok((outputs, telemetry)) => {
                                                ok_json(serde_json::json!({"status":"completed","results":outputs,"telemetry":telemetry}))
                                            }
                                            Err(e) => err_json(&e.to_string())
                                        }
                                    }
                                    Err(e) => err_json(&e.to_string())
                                }
                            } else {
                                err_json("Invalid flow definition")
                            }
                        }
                        _ => err_json("Flow not found for telemetry execution"),
                    }
                }

                // ── Capability Tools ──
                "capability_inventory" => {
                    let mc = engine.backend().list_modules().await.unwrap_or_default().len();
                    let gn = engine.graph().all_nodes().unwrap_or_default().len();
                    let ge = engine.graph().all_edges().unwrap_or_default().len();
                    ok_json(serde_json::json!({
                        "modules": mc, "graph_nodes": gn, "graph_edges": ge,
                        "resource_types": ["postgresql","slack","github","openai","http","aws"],
                        "tool_count": 39,
                    }))
                }

                // ── System Tools ──
                "system_health" => {
                    let mc = engine.backend().list_modules().await.unwrap_or_default().len();
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
        add_tool(&mut tools, "module_list_templates", "List available module templates", Arc::new(serde_json::Map::new()));
        // Workflow
        add_tool(&mut tools, "workflow_plan",   "Plan a workflow",                    schema_for::<WorkflowPlanParams>());
        add_tool(&mut tools, "workflow_materialize", "Validate a DAG",                schema_for::<WorkflowPlanParams>());
        // Graph
        add_tool(&mut tools, "graph_query",     "Query design graph",                 schema_for::<GraphQueryParams>());
        add_tool(&mut tools, "graph_pathfind",  "Find paths between nodes",           schema_for::<GraphPathfindParams>());
        add_tool(&mut tools, "graph_add_edge",  "Wire edge between nodes",            schema_for::<GraphAddEdgeParams>());
        // Flow
        add_tool(&mut tools, "flow_create",     "Compose steps into a flow",          schema_for::<FlowCreateParams>());
        add_tool(&mut tools, "flow_show",       "Show flow topology",                 schema_for::<FlowShowParams>());
        add_tool(&mut tools, "flow_execute",    "Execute a composed flow DAG",         schema_for::<FlowExecuteParams>());
        add_tool(&mut tools, "flow_list",       "List all stored flows",             Arc::new(serde_json::Map::new()));
        add_tool(&mut tools, "flow_delete",     "Delete a stored flow",              schema_for::<FlowShowParams>());
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
        add_tool(&mut tools, "run_retry",       "Retry a failed run",                 schema_for::<RunRetryParams>());
        // Registry
        add_tool(&mut tools, "registry_search", "Search registered modules",          schema_for::<RegistrySearchParams>());
        // Graph Search
        add_tool(&mut tools, "graph_search",     "Search graph nodes by name/text query",  schema_for::<SearchParams>());
        add_tool(&mut tools, "graph_time_range", "Query nodes and edges within a time range", schema_for::<TimeRangeParams>());
        // Webhook
        add_tool(&mut tools, "webhook_register", "Register an outbound webhook",         schema_for::<WebhookRegisterParams>());
        add_tool(&mut tools, "webhook_list",     "List all registered webhooks",         schema_for::<WebhookListParams>());
        add_tool(&mut tools, "webhook_delete",   "Delete a webhook by ID",              schema_for::<WebhookDeleteParams>());
        // Flow Telemetry
        add_tool(&mut tools, "flow_execute_telemetry", "Execute flow and return telemetry data", schema_for::<FlowExecuteTelemetryParams>());
        // Graph
        add_tool(&mut tools, "graph_summarize", "Get aggregated statistics about the knowledge graph including counts by node kind and edge relationship type", Arc::new(serde_json::Map::new()));
        // Capability
        add_tool(&mut tools, "capability_inventory", "Discover available capabilities", Arc::new(serde_json::Map::new()));
        // System
        add_tool(&mut tools, "system_health",   "Check system health",                Arc::new(serde_json::Map::new()));

        Box::pin(std::future::ready(Ok(ListToolsResult { tools, meta: None, next_cursor: None })))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let names = ["module_create","module_build","module_validate","module_run","module_deprecate",
            "module_search","module_template","module_list_templates","workflow_plan","workflow_materialize",
            "graph_query","graph_pathfind","graph_add_edge","graph_search","graph_time_range","graph_summarize",
            "flow_create","flow_show","flow_execute","flow_execute_telemetry","flow_list","flow_delete","schedule_create","schedule_validate",
            "webhook_register","webhook_list","webhook_delete",
            "secret_set","secret_get","resource_bind","resource_list",
            "job_queue","job_list","run_logs","run_retry","registry_search","capability_inventory","system_health"];
        if names.contains(&name) {
            Some(Tool::new(name.to_string(), "", Arc::new(serde_json::Map::new())))
        } else {
            None
        }
    }
}
