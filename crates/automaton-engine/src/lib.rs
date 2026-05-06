use std::collections::HashMap;

use automaton_core::*;
use automaton_graph::GraphStore;
use automaton_registry::Registry;
use automaton_runtime::Runtime;
use petgraph::graph::DiGraph;
use petgraph::algo::toposort;
use petgraph::prelude::*;

/// Options for planning a workflow.
#[derive(Debug, Clone)]
pub struct PlanOptions {
    pub max_depth: usize,
    pub include_alternatives: bool,
    pub dry_run: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self { max_depth: 10, include_alternatives: false, dry_run: false }
    }
}

/// The engine orchestrates planning, materialization, and execution.
pub struct Engine {
    registry: Registry,
    graph: GraphStore,
    runtime: Runtime,
}

impl Engine {
    pub fn new(registry: Registry, graph: GraphStore, runtime: Runtime) -> Self {
        Self { registry, graph, runtime }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn graph(&self) -> &GraphStore {
        &self.graph
    }

    /// Plan a workflow: discover the dependency graph from the registry.
    pub fn plan(&self, start_module: &str, options: &PlanOptions) -> Result<RunGraph> {
        let _has_module = self.registry.get(start_module)?
            .ok_or_else(|| AutomatonError::ModuleNotFound(start_module.to_string()))?;

        let run_id = uuid::Uuid::new_v4().to_string();
        let mut modules = vec![];
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![(start_module.to_string(), 0)];

        while let Some((path, depth)) = stack.pop() {
            if !visited.insert(path.clone()) || depth > options.max_depth {
                continue;
            }

            if let Some(mod_data) = self.registry.get(&path)? {
                let deps: Vec<String> = mod_data.manifest.depends_on.iter().map(|d| d.name.clone()).collect();

                let node = ModuleNode {
                    id: format!("{}-{}", path.replace('.', "-"),
                        uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0")),
                    module_id: ModuleId {
                        path: path.clone(),
                        version: semver::Version::parse(&mod_data.manifest.version)
                            .map_err(|e: semver::Error| AutomatonError::Other(e.to_string()))?,
                        hash: mod_data.hash.clone(),
                        created_at: chrono::Utc::now(),
                    },
                    input: serde_json::json!({}),
                    retry: mod_data.manifest.retry.clone(),
                    timeout_ms: mod_data.manifest.timeout_ms,
                    depends_on: deps.clone(),
                    parallel_group: None,
                    condition: None,
                    error_handler: None,
                };
                modules.push(node);

                for dep in &mod_data.manifest.depends_on {
                    stack.push((dep.name.clone(), depth + 1));
                }
            }
        }

        Ok(RunGraph {
            id: run_id,
            workflow_name: start_module.to_string(),
            modules,
            steps: vec![],
            created_at: chrono::Utc::now(),
        })
    }

    /// Materialize a run graph into an executable DAG, verifying acyclicity.
    pub fn materialize(&self, run_graph: &RunGraph) -> Result<ExecutableDag> {
        let mut dag = DiGraph::<ExecNode, ()>::new();
        let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();

        for module in &run_graph.modules {
            let idx = dag.add_node(ExecNode {
                id: module.id.clone(),
                module_name: module.module_id.path.clone(),
                input: module.input.clone(),
                retry: module.retry.clone(),
                timeout_ms: module.timeout_ms,
                state: ExecutionState::Pending,
            });
            node_indices.insert(module.id.clone(), idx);
        }

        for module in &run_graph.modules {
            let target_idx = match node_indices.get(&module.id) {
                Some(idx) => *idx,
                None => continue,
            };
            for dep in &module.depends_on {
                if let Some(found) = run_graph.modules.iter().find(|m| m.module_id.path == *dep)
                    && let Some(source_idx) = node_indices.get(&found.id) {
                        dag.add_edge(*source_idx, target_idx, ());
                    }
            }
        }

        match toposort(&dag, None) {
            Ok(_) => {}
            Err(_) => return Err(AutomatonError::CyclicDependency),
        }

        Ok(ExecutableDag { graph: dag, node_indices, run_graph_id: run_graph.id.clone() })
    }

    /// Execute a materialized DAG.
    /// Uses a separate states HashMap to avoid borrow conflicts on the graph.
    pub async fn execute(&self, mut dag: ExecutableDag) -> Result<RunResult> {
        let order = toposort(&dag.graph, None)
            .map_err(|_| AutomatonError::CyclicDependency)?;

        let mut results: HashMap<String, serde_json::Value> = HashMap::new();
        let mut states: HashMap<NodeIndex, ExecutionState> = HashMap::new();

        for node_idx in &order {
            let _exec_node = &dag.graph[*node_idx];
            states.insert(*node_idx, ExecutionState::Pending);
        }

        for node_idx in &order {
            // Read-only borrow to inspect graph
            let (exec_id, module_path, input, retry, timeout) = {
                let n = &dag.graph[*node_idx];
                (n.id.clone(), n.module_name.clone(), n.input.clone(), n.retry.clone(), n.timeout_ms)
            };

            tracing::info!(module = %module_path, "Executing module");
            self.registry.record_run(&exec_id, &module_path, &input)?;

            // Check that all incoming deps completed
            let mut deps_satisfied = true;
            for incoming in dag.graph.neighbors_directed(*node_idx, Incoming) {
                match states.get(&incoming) {
                    Some(ExecutionState::Completed(_)) => {}
                    _ => { deps_satisfied = false; break; }
                }
            }

            if !deps_satisfied {
                tracing::warn!(module = %module_path, "Skipping — dependencies not satisfied");
                states.insert(*node_idx, ExecutionState::Skipped("Unsatisfied dependencies".into()));
                continue;
            }

            let build_cache_dir = self.registry.build_cache_dir();
            let binary_path = build_cache_dir.join(module_path.replace('.', "_"));

            let result = if binary_path.exists() {
                if let Some(retry_cfg) = &retry {
                    self.runtime.run_with_retry(&binary_path, &input, retry_cfg, timeout).await
                } else {
                    self.runtime.run_binary(&binary_path, &input, timeout).await
                }
            } else {
                Err(AutomatonError::Other("No compiled binary".into()))
            };

            match result {
                Ok(output) => {
                    tracing::info!(module = %module_path, "Completed");
                    self.registry.update_run(&exec_id, "completed", Some(&output), None, 1)?;
                    states.insert(*node_idx, ExecutionState::Completed(output.clone()));
                    results.insert(module_path, output);
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    tracing::error!(module = %module_path, error = %err_msg, "Failed");
                    self.registry.update_run(&exec_id, "failed", None, Some(&err_msg), 1)?;
                    states.insert(*node_idx, ExecutionState::Failed(err_msg.clone()));
                    results.insert(module_path, serde_json::json!({"error": err_msg}));
                }
            }
        }

        // Write final states back to graph
        for (idx, state) in &states {
            dag.graph[*idx].state = state.clone();
        }

        Ok(RunResult {
            run_graph_id: dag.run_graph_id,
            results,
            completed_at: chrono::Utc::now(),
        })
    }
}

/// A node in the executable DAG.
#[derive(Debug, Clone)]
pub struct ExecNode {
    pub id: String,
    pub module_name: String,
    pub input: serde_json::Value,
    pub retry: Option<RetryConfig>,
    pub timeout_ms: u64,
    pub state: ExecutionState,
}

/// A materialized executable DAG backed by petgraph.
pub struct ExecutableDag {
    pub graph: DiGraph<ExecNode, ()>,
    pub node_indices: HashMap<String, NodeIndex>,
    pub run_graph_id: String,
}

/// Result of a DAG execution.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunResult {
    pub run_graph_id: String,
    pub results: HashMap<String, serde_json::Value>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}
