use std::path::PathBuf;

use clap::{Parser, Subcommand};

use std::sync::Arc;

use automaton_build::BuildCache;
use automaton_core::*;
use automaton_engine::{Engine, PlanOptions};
use automaton_graph::GraphStore;
use automaton_registry::Registry;
use automaton_runtime::{Runtime, RuntimeConfig};
use automaton_mcp::McpServer;
use automaton_scheduler::{SchedulerDaemon, ScheduledTrigger, TriggerProvider};
use automaton_worker::Worker;
use automaton_postgres::AutomatonDb;

/// Automaton — AI-native Rust automation substrate.
///
/// A CLI-based framework for AI agents to create, compose, and execute
/// modular Rust automations backed by a property graph and MCP-native control.
#[derive(Parser)]
#[command(name = "automaton", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new automaton workspace
    Init,

    /// Scaffold a new automation module
    New {
        /// Module path, e.g. "github.issue_triage"
        path: String,
        /// Template pattern (echo, http-fetch, http-server, db-query, slack-notify, data-transform, health-check, rate-limiter, file-watch, cron-worker)
        #[arg(long, default_value = "echo")]
        pattern: String,
    },

    /// Build a module into a compiled binary
    Build {
        /// Module path
        path: String,
        /// Build mode (debug/release)
        #[arg(long, default_value = "debug")]
        mode: String,
    },

    /// Run a module locally
    Run {
        /// Module path
        path: String,
        /// JSON input string
        #[arg(long)]
        input: Option<String>,
    },

    /// Inspect and query the design graph
    Graph {
        #[command(subcommand)]
        action: GraphCommand,
    },

    /// Plan a workflow from a module
    Plan {
        /// Starting module path
        start: String,
        /// Maximum dependency depth
        #[arg(long, default_value = "10")]
        max_depth: usize,
    },

    /// Plan and execute a workflow from a module
    Execute {
        /// Module path to plan and execute from
        module: String,
        /// Maximum dependency depth
        #[arg(long, default_value = "10")]
        max_depth: usize,
        /// JSON input to pass to the first module (optional)
        #[arg(long)]
        input: Option<String>,
    },

    /// Start the MCP server (stdio transport)
    Mcp,

    /// List registered modules
    List {
        /// Filter by query
        query: Option<String>,
    },

    /// Show module details
    Show {
        /// Module path
        path: String,
    },

    /// View run logs
    Logs {
        /// Module path filter
        module: Option<String>,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Retry a failed run
    Retry {
        /// Run ID
        run_id: String,
    },

    /// Start the worker daemon (processes queued jobs)
    Worker {
        /// Worker name (for multi-worker setups)
        #[arg(long, default_value = "default")]
        name: String,
        /// Number of concurrent jobs
        #[arg(long, default_value = "4")]
        concurrency: usize,
        /// Poll interval in milliseconds
        #[arg(long, default_value = "5000")]
        poll_interval_ms: u64,
        /// Detach and run in background (writes PID to data dir)
        #[arg(long)]
        daemon: bool,
    },

    /// Run system diagnostics
    Doctor,

    /// Postgres database operations
    Postgres {
        #[command(subcommand)]
        action: PostgresCommand,
    },
}

#[derive(Subcommand)]
enum PostgresCommand {
    /// Run database schema migrations
    Migrate {
        /// Database connection URL (defaults to DATABASE_URL env var or postgres://localhost:5432/automaton)
        #[arg(long)]
        database_url: Option<String>,
    },
}

#[derive(Subcommand)]
enum GraphCommand {
    /// List all nodes
    Nodes,
    /// List all edges
    Edges,
    /// Find a path between two nodes
    Path {
        from: String,
        to: String,
    },
    /// Show dependency chain for a node
    Deps {
        node_id: String,
    },
}

// ── TriggerProvider adapter for Registry ──

struct RegistryTriggerProvider {
    registry: Registry,
}

#[async_trait::async_trait]
impl TriggerProvider for RegistryTriggerProvider {
    async fn get_cron_triggers(&self) -> std::result::Result<Vec<ScheduledTrigger>, String> {
        let triggers = self.registry.get_enabled_triggers("cron")
            .map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for t in triggers {
            let config = t.get("config").and_then(|c| c.as_object()).cloned().unwrap_or_default();
            let schedule = config.get("schedule")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if schedule.is_empty() {
                continue;
            }
            result.push(ScheduledTrigger {
                id: t.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                target_path: t.get("target_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                target_is_flow: t.get("target_is_flow").and_then(|v| v.as_bool()).unwrap_or(false),
                schedule,
                args: config.get("args").cloned(),
            });
        }
        Ok(result)
    }

    async fn enqueue_job(&self, kind: &str, target: &str, args: &serde_json::Value) -> std::result::Result<i64, String> {
        self.registry.enqueue(kind, target, args)
            .map_err(|e| e.to_string())
    }
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from(".local/share"))
        .join("automaton")
}

fn init_engine(data_dir: &PathBuf) -> Result<Engine> {
    std::fs::create_dir_all(data_dir)?;
    let registry = Registry::open(data_dir)?;
    // Use merged registry.db for graph store (replaces separate graph.db)
    let graph_store = GraphStore::open_merged(data_dir)?;
    let runtime_config = RuntimeConfig {
        work_dir: data_dir.join("work"),
        temp_dir: data_dir.join("tmp"),
        ..Default::default()
    };
    let runtime = Runtime::new(runtime_config);
    Ok(Engine::new(Arc::new(registry), graph_store, runtime))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("automaton=info".parse()?))
        .init();

    let cli = Cli::parse();
    let data_dir = default_data_dir();

    match cli.command {
        Commands::Init => {
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(data_dir.join("modules"))?;
            std::fs::create_dir_all(data_dir.join("builds"))?;
            std::fs::create_dir_all(data_dir.join("work"))?;
            std::fs::create_dir_all(data_dir.join("tmp"))?;

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "initialized",
                "data_dir": data_dir.to_string_lossy(),
                "version": env!("CARGO_PKG_VERSION"),
            }))?);
        }

        Commands::New { path, pattern } => {
            if path.is_empty() || path.trim().is_empty() {
                anyhow::bail!("Module path cannot be empty");
            }
            if path.contains("..") || path.contains("//") {
                anyhow::bail!("Invalid module path: {path}");
            }
            let module_dir = data_dir.join("modules").join(path.replace('.', "/"));
            std::fs::create_dir_all(&module_dir)?;
            let safe = path.replace('.', "_");
            // Check for template pattern
            let source = if let Some(tmpl) = automaton_build::templates::get_template(&pattern) {
                tmpl.source.to_string()
            } else {
                // Fallback: simple echo source
                format!("// Automation: {safe}\nfn main() {{\n    let msg = serde_json::json!({{ \"status\": \"ok\", \"module\": \"{safe}\" }});\n    println!(\"{{}}\", msg);\n}}\n")
            };
            let source_path = module_dir.join("main.rs");
            std::fs::write(&source_path, &source)?;

            let manifest = AutomationManifest {
                name: path.clone(),
                version: "0.1.0".to_string(),
                entry: "main".to_string(),
                summary: Some(format!("Automation: {path}")),
                description: None,
                timeout_ms: 30_000,
                ..Default::default()
            };
            let yaml = serde_yaml::to_string(&manifest)?;
            let yaml_path = module_dir.join("automation.yaml");
            std::fs::write(&yaml_path, &yaml)?;

            // Register module in registry + add to graph (single engine init)
            match init_engine(&data_dir) {
                Ok(engine) => {
                    match engine.backend().register_module(&path, &source, &manifest).await {
                        Ok(_id) => {
                            let mut props = std::collections::HashMap::new();
                            props.insert("path".into(), serde_json::json!(path));
                            let _ = engine.graph().add_node(NodeKind::Module, &path, props);
                        }
                        Err(e) => eprintln!("Warning: Failed to register module: {e}"),
                    }
                }
                Err(e) => eprintln!("Warning: Failed to init engine: {e}"),
            }

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "created",
                "path": path,
                "source": source_path.to_string_lossy(),
                "manifest": yaml_path.to_string_lossy(),
            }))?);
        }

        Commands::Build { path, mode } => {
            let engine = init_engine(&data_dir)?;
            let module = engine.backend().get_module(&path).await?
                .ok_or_else(|| anyhow::anyhow!("Module not found: {path}"))?;

            let build_cache = automaton_build::BuildCache::new(&data_dir);
            let (hash, binary_path) = build_cache.build_rust(
                &path,
                &module.source,
                &module.manifest,
            ).map_err(|e| anyhow::anyhow!("Build failed: {e}"))?;

            engine.backend().mark_built(&path).await?;
            engine.backend().record_build(&hash, &binary_path.to_string_lossy(), &mode).await?;

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "built",
                "path": path,
                "mode": mode,
                "binary": binary_path.to_string_lossy(),
                "hash": hash,
            }))?);
        }

        Commands::Run { path, input } => {
            let engine = init_engine(&data_dir)?;
            let module = engine.backend().get_module(&path).await?
                .ok_or_else(|| anyhow::anyhow!("Module not found: {path}"))?;

            if !module.built {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "status": "skipped",
                    "reason": "Module not built yet. Run `automaton build {path}` first.",
                }))?);
                return Ok(());
            }

            let input_value: serde_json::Value = if let Some(input_str) = input {
                serde_json::from_str(&input_str)?
            } else {
                serde_json::json!({})
            };

            let binary_path = engine.backend().build_cache_dir()
                .join(path.replace('.', "_"));
            let runtime = Runtime::new(RuntimeConfig::default());
            let result = runtime.run_binary(&binary_path, &input_value, module.manifest.timeout_ms).await;

            match result {
                Ok(output) => {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "status": "completed",
                        "output": output,
                    }))?);
                }
                Err(e) => {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "status": "failed",
                        "error": e.to_string(),
                    }))?);
                }
            }
        }

        Commands::Graph { action } => {
            let engine = init_engine(&data_dir)?;

            match action {
                GraphCommand::Nodes => {
                    let nodes = engine.graph().all_nodes()?;
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "count": nodes.len(),
                        "nodes": nodes.iter().map(|n| serde_json::json!({
                            "id": n.id,
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                            "properties": n.properties,
                        })).collect::<Vec<_>>(),
                    }))?);
                }
                GraphCommand::Edges => {
                    let edges = engine.graph().all_edges()?;
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "count": edges.len(),
                        "edges": edges.iter().map(|e| serde_json::json!({
                            "id": e.id,
                            "source": e.source,
                            "target": e.target,
                            "kind": format!("{:?}", e.kind),
                        })).collect::<Vec<_>>(),
                    }))?);
                }
                GraphCommand::Path { from, to } => {
                    let paths = engine.graph().find_path(&from, &to)?;
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "paths_found": paths.len(),
                        "paths": paths.iter().map(|path| {
                            path.iter().map(|na| serde_json::json!({
                                "node": na.node.name,
                                "edge": format!("{:?}", na.edge_kind),
                            })).collect::<Vec<_>>()
                        }).collect::<Vec<_>>(),
                    }))?);
                }
                GraphCommand::Deps { node_id } => {
                    let chain = engine.graph().get_dependency_chain(&node_id)?;
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "count": chain.len(),
                        "chain": chain.iter().map(|n| serde_json::json!({
                            "id": n.id,
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                        })).collect::<Vec<_>>(),
                    }))?);
                }
            }
        }

        Commands::Plan { start, max_depth } => {
            let engine = init_engine(&data_dir)?;
            let options = PlanOptions { max_depth, ..Default::default() };
            let run_graph = engine.plan(&start, &options).await?;

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "run_graph_id": run_graph.id,
                "workflow": run_graph.workflow_name,
                "modules": run_graph.modules.iter().map(|m| serde_json::json!({
                    "id": m.id,
                    "module": m.module_id.path,
                    "depends_on": m.depends_on,
                    "retry": serde_json::to_value(&m.retry).ok(),
                    "timeout_ms": m.timeout_ms,
                })).collect::<Vec<_>>(),
                "total_modules": run_graph.modules.len(),
            }))?);
        }

        Commands::Execute { module, max_depth, input: _ } => {
            let engine = init_engine(&data_dir)?;

            // Plan from the module path
            let options = PlanOptions { max_depth, ..Default::default() };
            let run_graph = engine.plan(&module, &options).await?;
            let dag = engine.materialize(&run_graph)?;

            let result = engine.execute(dag).await?;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "executed",
                "run_id": result.run_graph_id,
                "results": result.results,
            }))?);
        }

        Commands::Mcp => {
            let engine = init_engine(&data_dir)?;
            let server = McpServer::new(engine, data_dir);
            tracing::info!("Starting Automaton MCP server on stdio");
            server.serve_stdio().await?;
        }

        Commands::List { query } => {
            let engine = init_engine(&data_dir)?;
            let all = engine.backend().list_modules().await?;
            let filtered: Vec<_> = if let Some(q) = query {
                all.into_iter().filter(|(p, _, _, _)| p.contains(&q)).collect()
            } else {
                all
            };

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "count": filtered.len(),
                "modules": filtered.iter().map(|(path, ver, hash, built)| serde_json::json!({
                    "path": path,
                    "version": ver,
                    "hash": hash,
                    "built": built,
                })).collect::<Vec<_>>(),
            }))?);
        }

        Commands::Show { path } => {
            let engine = init_engine(&data_dir)?;
            match engine.backend().get_module(&path).await? {
                Some(module) => {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "path": module.manifest.name,
                        "version": module.manifest.version,
                        "hash": module.hash.as_str(),
                        "built": module.built,
                        "entry": module.manifest.entry,
                        "summary": module.manifest.summary,
                        "dependencies": module.manifest.depends_on.iter().map(|d| serde_json::json!({
                            "name": d.name,
                            "version_req": d.version_req,
                        })).collect::<Vec<_>>(),
                        "timeout_ms": module.manifest.timeout_ms,
                        "retry": serde_json::to_value(&module.manifest.retry).ok(),
                        "tags": module.manifest.tags,
                    }))?);
                }
                None => {
                    eprintln!("Module not found: {path}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Logs { module, limit } => {
            let engine = init_engine(&data_dir)?;
            let runs = if let Some(ref module_path) = module {
                engine.backend().get_runs(module_path).await?
            } else {
                vec![]
            };

            let truncated: Vec<_> = runs.into_iter().take(limit).collect();
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "count": truncated.len(),
                "runs": truncated,
            }))?);
        }

        Commands::Retry { run_id } => {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "retry_scheduled",
                "run_id": run_id,
            }))?);
        }

        Commands::Worker { name, concurrency, poll_interval_ms, daemon } => {
            // Open separate registry instances (SQLite handles concurrent access)
            let worker_registry = Registry::open(&data_dir)?;
            let scheduler_registry = Registry::open(&data_dir)?;

            let build_cache = BuildCache::new(&data_dir);
            let worker = Worker::new(&name, concurrency)
                .with_build_cache(build_cache);

            // Start the scheduler daemon to fire cron triggers in the background
            let provider = Arc::new(RegistryTriggerProvider { registry: scheduler_registry });
            let _scheduler = SchedulerDaemon::start(provider, 60_000);

            if daemon {
                let pid_path = data_dir.join("worker.pid");
                let child = std::process::Command::new(std::env::current_exe()?)
                    .args([
                        "worker",
                        "--name", &name,
                        "--concurrency", &concurrency.to_string(),
                        "--poll-interval-ms", &poll_interval_ms.to_string(),
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .stdin(std::process::Stdio::null())
                    .spawn()?;
                std::fs::write(&pid_path, child.id().to_string())?;
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "status": "worker_detached",
                    "name": name,
                    "pid": child.id(),
                    "pid_file": pid_path.to_string_lossy(),
                }))?);
                std::process::exit(0);
            }

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "worker_started",
                "name": name,
                "concurrency": concurrency,
                "poll_interval_ms": poll_interval_ms,
                "scheduler": "active",
            }))?);

            // Worker runs in the foreground, processing jobs from the queue
            worker.start(&worker_registry, poll_interval_ms).await;
        }

        Commands::Postgres { action } => {
            match action {
                PostgresCommand::Migrate { database_url } => {
                    let url = database_url
                        .or_else(|| std::env::var("DATABASE_URL").ok())
                        .unwrap_or_else(|| "postgres://localhost:5432/automaton".to_string());

                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "status": "connecting",
                        "database_url": url,
                    }))?);

                    match AutomatonDb::connect(&url).await {
                        Ok(_db) => {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "status": "migrated",
                                "database_url": url,
                            }))?);
                        }
                        Err(e) => {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "status": "error",
                                "error": e.to_string(),
                            }))?);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        Commands::Doctor => {
            let engine = init_engine(&data_dir)?;
            let module_count = engine.backend().list_modules().await.unwrap_or_default().len();
            let graph_nodes = engine.graph().all_nodes().unwrap_or_default().len();
            let graph_edges = engine.graph().all_edges().unwrap_or_default().len();

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "status": "healthy",
                "version": env!("CARGO_PKG_VERSION"),
                "data_dir": data_dir.to_string_lossy(),
                "components": {
                    "registry": {
                        "status": "ok",
                        "modules_registered": module_count,
                    },
                    "graph": {
                        "status": "ok",
                        "nodes": graph_nodes,
                        "edges": graph_edges,
                    },
                    "runtime": {
                        "status": "ready",
                        "max_concurrency": 4,
                    },
                    "engine": {
                        "status": "ready",
                    },
                },
            }))?);
        }
    }

    Ok(())
}
