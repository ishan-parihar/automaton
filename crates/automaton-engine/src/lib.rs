pub mod flow;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use automaton_core::backend::RegistryBackend;
use automaton_core::*;
use automaton_graph::GraphStore;
use automaton_runtime::Runtime;
use futures::future::join_all;
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
    backend: Arc<dyn RegistryBackend>,
    graph: GraphStore,
    runtime: Runtime,
}

impl Engine {
    pub fn new(backend: Arc<dyn RegistryBackend>, graph: GraphStore, runtime: Runtime) -> Self {
        Self { backend, graph, runtime }
    }

    pub fn backend(&self) -> &Arc<dyn RegistryBackend> {
        &self.backend
    }

    pub fn registry_deprecated(&self) -> &Arc<dyn RegistryBackend> {
        &self.backend
    }

    pub fn graph(&self) -> &GraphStore {
        &self.graph
    }

    /// Plan a workflow: discover the dependency graph from the registry.
    pub async fn plan(&self, start_module: &str, options: &PlanOptions) -> Result<RunGraph> {
        let has_module = self.backend.get_module(start_module).await?
            .ok_or_else(|| AutomatonError::ModuleNotFound(start_module.to_string()))?;
        let _ = has_module;

        let run_id = uuid::Uuid::new_v4().to_string();
        let mut modules = vec![];
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![(start_module.to_string(), 0)];

        while let Some((path, depth)) = stack.pop() {
            if !visited.insert(path.clone()) || depth > options.max_depth {
                continue;
            }

            if let Some(mod_data) = self.backend.get_module(&path).await? {
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

    /// Execute a materialized DAG with level-based parallelism.
    /// Nodes at the same topological level (no dependency between them) run concurrently.
    /// Cross-step state is accumulated in a shared map for downstream reference.
    /// Input templates like `${module_path}` or `${module_path.field}` are resolved
    /// against upstream results before execution.
    pub async fn execute(&self, mut dag: ExecutableDag) -> Result<RunResult> {
        let order = toposort(&dag.graph, None)
            .map_err(|_| AutomatonError::CyclicDependency)?;

        // Compute topological levels (max depth from root nodes)
        let mut levels: HashMap<NodeIndex, usize> = HashMap::new();
        for &node_idx in &order {
            let lvl = dag.graph.neighbors_directed(node_idx, Incoming)
                .filter_map(|incoming| levels.get(&incoming))
                .max()
                .map(|l| l + 1)
                .unwrap_or(0);
            levels.insert(node_idx, lvl);
        }

        // Group nodes by level for parallel execution
        let mut level_groups: BTreeMap<usize, Vec<NodeIndex>> = BTreeMap::new();
        for (&node_idx, &lvl) in &levels {
            level_groups.entry(lvl).or_default().push(node_idx);
        }

        let run_graph_id = dag.run_graph_id.clone();
        // flow_state accumulates outputs keyed by module path for downstream reference
        let mut flow_state: HashMap<String, serde_json::Value> = HashMap::new();
        let mut states: HashMap<NodeIndex, ExecutionState> = HashMap::new();

        // Initialise all states to Pending
        for &node_idx in &order {
            states.insert(node_idx, ExecutionState::Pending);
        }

        // Execute level by level — all nodes within a level run in parallel
        let build_cache_dir = self.backend.build_cache_dir();
        for group in level_groups.values() {
            // Resolve $var:/$res: references and ${state} references in inputs
            let mut resolved_inputs = Vec::with_capacity(group.len());
            for &node_idx in group {
                let input = &dag.graph[node_idx].input;
                let with_vars = self.backend.resolve_references(input)
                    .await
                    .unwrap_or_else(|_| input.clone());
                let resolved = resolve_state_refs(&with_vars, &flow_state);
                resolved_inputs.push(resolved);
            }

            let futs: Vec<_> = group.iter().zip(resolved_inputs.iter()).map(|(&node_idx, resolved_input)| {
                let exec_id = dag.graph[node_idx].id.clone();
                let module_path = dag.graph[node_idx].module_name.clone();
                let input = resolved_input.clone();
                let retry = dag.graph[node_idx].retry.clone();
                let timeout = dag.graph[node_idx].timeout_ms;
                let build_cache_dir = build_cache_dir.clone();

                async move {
                    tracing::info!(module = %module_path, "Executing module");

                    let binary_path = build_cache_dir.join(module_path.replace('.', "_"));

                    if binary_path.exists() {
                        let result = if let Some(retry_cfg) = &retry {
                            self.runtime.run_with_retry(&binary_path, &input, retry_cfg, timeout).await
                        } else {
                            self.runtime.run_binary(&binary_path, &input, timeout).await
                        };
                        (node_idx, exec_id, module_path, result)
                    } else {
                        (node_idx, exec_id, module_path, Err(AutomatonError::Other("No compiled binary".into())))
                    }
                }
            }).collect();

            let outcomes: Vec<(NodeIndex, String, String, std::result::Result<serde_json::Value, AutomatonError>)> = join_all(futs).await;

            for (node_idx, exec_id, module_path, result) in outcomes {
                match result {
                    Ok(output) => {
                        tracing::info!(module = %module_path, "Completed");
                        let _ = self.backend.record_run(&exec_id, &module_path, &serde_json::json!({})).await;
                        let _ = self.backend.update_run(&exec_id, "completed", Some(&output), None, 1).await;
                        states.insert(node_idx, ExecutionState::Completed(output.clone()));
                        flow_state.insert(module_path, output);
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        tracing::error!(module = %module_path, error = %err_msg, "Failed");
                        let _ = self.backend.record_run(&exec_id, &module_path, &serde_json::json!({})).await;
                        let _ = self.backend.update_run(&exec_id, "failed", None, Some(&err_msg), 1).await;
                        states.insert(node_idx, ExecutionState::Failed(err_msg.clone()));
                        flow_state.insert(module_path, serde_json::json!({"error": err_msg}));
                    }
                }
            }
        }

        // Write final states back to the graph
        for (idx, state) in &states {
            dag.graph[*idx].state = state.clone();
        }

        Ok(RunResult {
            run_graph_id,
            results: flow_state.clone(),
            flow_state: flow_state.clone(),
            completed_at: chrono::Utc::now(),
        })
    }
}

/// Resolve state references of the form `${module_path}` or `${module_path.field}`
/// in a JSON value, using the accumulated flow_state from completed modules.
pub fn resolve_state_refs(val: &serde_json::Value, state: &HashMap<String, serde_json::Value>) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => {
            if s.starts_with("${") && s.ends_with("}") {
                let inner = &s[2..s.len()-1];
                if let Some(dot_pos) = inner.find('.') {
                    let module_part = &inner[..dot_pos];
                    let field_part = &inner[dot_pos+1..];
                    if let Some(module_output) = state.get(module_part) {
                        if let Some(field_val) = module_output.get(field_part) {
                            return field_val.clone();
                        }
                        return serde_json::Value::Null;
                    }
                    return val.clone();
                }
                // No dot — reference entire module output
                if let Some(module_output) = state.get(inner) {
                    return module_output.clone();
                }
            }
            val.clone()
        }
        serde_json::Value::Object(map) => {
            let mut resolved = serde_json::Map::new();
            for (k, v) in map {
                resolved.insert(k.clone(), resolve_state_refs(v, state));
            }
            serde_json::Value::Object(resolved)
        }
        serde_json::Value::Array(arr) => {
            let resolved: Vec<_> = arr.iter().map(|v| resolve_state_refs(v, state)).collect();
            serde_json::Value::Array(resolved)
        }
        other => other.clone(),
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

/// Result of a DAG execution with cross-step flow state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunResult {
    pub run_graph_id: String,
    /// Per-module results keyed by module path (backward compat alias)
    pub results: HashMap<String, serde_json::Value>,
    /// Accumulated flow state — same data as `results` but named for clarity
    pub flow_state: HashMap<String, serde_json::Value>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_state_refs_direct() {
        let mut state = HashMap::new();
        state.insert("step_a".to_string(), serde_json::json!({"value": 42}));

        let result = resolve_state_refs(&serde_json::json!("${step_a}"), &state);
        assert_eq!(result, serde_json::json!({"value": 42}));

        let result = resolve_state_refs(&serde_json::json!("${step_a.value}"), &state);
        assert_eq!(result, serde_json::json!(42));

        let result = resolve_state_refs(&serde_json::json!("hello"), &state);
        assert_eq!(result, serde_json::json!("hello"));

        let result = resolve_state_refs(&serde_json::json!("${missing}"), &state);
        assert_eq!(result, serde_json::json!("${missing}"));
    }

    #[test]
    fn test_resolve_state_refs_nested_object() {
        let mut state = HashMap::new();
        state.insert("step_b".to_string(), serde_json::json!({"msg": "OK"}));

        let input = serde_json::json!({
            "data": "${step_b}",
            "fallback": "static_value",
        });
        let result = resolve_state_refs(&input, &state);
        assert_eq!(result["data"], serde_json::json!({"msg": "OK"}));
        assert_eq!(result["fallback"], "static_value");
    }

    #[test]
    fn test_resolve_state_refs_array() {
        let mut state = HashMap::new();
        state.insert("step_c".to_string(), serde_json::json!("result_c"));

        let input = serde_json::json!(["${step_c}", "literal", 123]);
        let result = resolve_state_refs(&input, &state);
        assert_eq!(result[0], "result_c");
        assert_eq!(result[1], "literal");
        assert_eq!(result[2], 123);
    }

    #[test]
    fn test_resolve_state_refs_missing_field() {
        let mut state = HashMap::new();
        state.insert("x".to_string(), serde_json::json!({"a": 1}));

        let result = resolve_state_refs(&serde_json::json!("${x.missing}"), &state);
        assert_eq!(result, serde_json::Value::Null);
    }
}
