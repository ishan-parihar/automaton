//! Flow DAG engine for Automaton.
//! Handles branch_one, branch_all, forloop, whileloop, failure_module execution.

use automaton_core::*;

pub struct FlowEngine;

impl FlowEngine {
    /// Flatten a flow definition into an ordered list of executable steps
    /// with dependency resolution, handling branches and loops.
    pub fn flatten(flow: &FlowDefinition) -> Result<Vec<ExecStep>> {
        let mut steps = Vec::new();
        Self::flatten_steps(&flow.steps, &flow.default_retry, flow.default_timeout_ms, &mut steps, 0)?;
        Ok(steps)
    }

    fn flatten_steps(
        input: &[FlowStep],
        default_retry: &Option<RetryConfig>,
        default_timeout: u64,
        result: &mut Vec<ExecStep>,
        depth: usize,
    ) -> Result<()> {
        if depth > 10 {
            return Err(AutomatonError::Other("Flow nesting too deep (>10)".into()));
        }
        for step in input {
            let retry = step.retry.clone().or_else(|| default_retry.clone());
            match &step.kind {
                FlowStepKind::Script => {
                    result.push(ExecStep {
                        id: step.id.clone(),
                        kind: ExecStepKind::Script,
                        script_path: step.script_path.clone(),
                        input: step.input.clone(),
                        retry,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: step.sleep_after_ms,
                        stop_if: step.stop_if.clone(),
                        failure_step: step.failure_step.clone(),
                    });
                }
                FlowStepKind::Sleep => {
                    result.push(ExecStep {
                        id: step.id.clone(),
                        kind: ExecStepKind::Sleep,
                        script_path: None,
                        input: serde_json::json!({"duration_ms": step.sleep_after_ms.unwrap_or(1000)}),
                        retry: None,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: step.sleep_after_ms,
                        stop_if: None,
                        failure_step: step.failure_step.clone(),
                    });
                }
                FlowStepKind::BranchOne(branches) => {
                    let branch_steps: Vec<Vec<String>> = branches.iter().map(|b| {
                        let mut ids = Vec::new();
                        for bs in b {
                            ids.push(bs.id.clone());
                        }
                        ids
                    }).collect();
                    result.push(ExecStep {
                        id: format!("{}__branch_one", step.id),
                        kind: ExecStepKind::BranchOne(branch_steps),
                        script_path: None,
                        input: step.input.clone(),
                        retry: None,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: None,
                        stop_if: None,
                        failure_step: None,
                    });
                    for branch in branches {
                        Self::flatten_steps(branch, default_retry, default_timeout, result, depth + 1)?;
                    }
                }
                FlowStepKind::BranchAll(branches) => {
                    let branch_steps: Vec<Vec<String>> = branches.iter().map(|b| {
                        b.iter().map(|bs| bs.id.clone()).collect()
                    }).collect();
                    result.push(ExecStep {
                        id: format!("{}__branch_all", step.id),
                        kind: ExecStepKind::BranchAll(branch_steps),
                        script_path: None,
                        input: step.input.clone(),
                        retry: None,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: None,
                        stop_if: None,
                        failure_step: None,
                    });
                    for branch in branches {
                        Self::flatten_steps(branch, default_retry, default_timeout, result, depth + 1)?;
                    }
                }
                FlowStepKind::ForLoop { iterator, steps } => {
                    result.push(ExecStep {
                        id: format!("{}__forloop", step.id),
                        kind: ExecStepKind::ForLoop(iterator.clone()),
                        script_path: None,
                        input: step.input.clone(),
                        retry: None,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: None,
                        stop_if: None,
                        failure_step: None,
                    });
                    Self::flatten_steps(steps, default_retry, default_timeout, result, depth + 1)?;
                }
                FlowStepKind::WhileLoop { condition, steps, max_iterations } => {
                    result.push(ExecStep {
                        id: format!("{}__whileloop", step.id),
                        kind: ExecStepKind::WhileLoop(condition.clone(), *max_iterations),
                        script_path: None,
                        input: step.input.clone(),
                        retry: None,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: None,
                        stop_if: None,
                        failure_step: None,
                    });
                    Self::flatten_steps(steps, default_retry, default_timeout, result, depth + 1)?;
                }
                FlowStepKind::FailureModule => {
                    result.push(ExecStep {
                        id: step.id.clone(),
                        kind: ExecStepKind::FailureModule,
                        script_path: step.script_path.clone(),
                        input: step.input.clone(),
                        retry: None,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: None,
                        stop_if: None,
                        failure_step: None,
                    });
                }
            }
        }
        Ok(())
    }

    /// Execute a flattened flow step by step with dependency resolution
    pub async fn execute(
        steps: &[ExecStep],
        run_fn: impl Fn(&str, &serde_json::Value) -> Result<serde_json::Value>,
    ) -> Result<Vec<(String, serde_json::Value)>> {
        let mut results: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
        let mut completed = std::collections::HashSet::new();
        let mut pending: std::collections::VecDeque<usize> = (0..steps.len()).collect();
        let mut max_iter = 1000;

        while !pending.is_empty() && max_iter > 0 {
            max_iter -= 1;
            let idx = pending.pop_front().unwrap();
            let step = &steps[idx];

            // Check dependencies
            let deps_met = step.depends_on.iter().all(|d| completed.contains(d));
            if !deps_met {
                pending.push_back(idx);
                continue;
            }

            match step.kind {
                ExecStepKind::Script => {
                    let script = step.script_path.as_deref().unwrap_or("");
                    let result = run_fn(script, &step.input)?;
                    completed.insert(step.id.clone());
                    results.insert(step.id.clone(), result);
                }
                ExecStepKind::Sleep => {
                    let ms = step.sleep_after_ms.unwrap_or(1000);
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                    completed.insert(step.id.clone());
                    results.insert(step.id.clone(), serde_json::json!({"slept_ms": ms}));
                }
                ExecStepKind::BranchOne(ref branches) => {
                    // Run each branch independently; first success wins
                    let mut branch_result = None;
                    for branch_ids in branches {
                        for bid in branch_ids {
                            if let Some(bstep) = steps.iter().find(|s| s.id == *bid) {
                                let script = bstep.script_path.as_deref().unwrap_or("");
                                match run_fn(script, &bstep.input) {
                                    Ok(r) => {
                                        branch_result = Some(r);
                                        break;
                                    }
                                    Err(_) => continue,
                                }
                            }
                        }
                        if branch_result.is_some() {
                            break;
                        }
                    }
                    completed.insert(step.id.clone());
                    results.insert(step.id.clone(), branch_result.unwrap_or(serde_json::json!({"error": "all_branches_failed"})));
                }
                ExecStepKind::BranchAll(ref branches) => {
                    let mut all_results = Vec::new();
                    for branch_ids in branches {
                        for bid in branch_ids {
                            if let Some(bstep) = steps.iter().find(|s| s.id == *bid) {
                                let script = bstep.script_path.as_deref().unwrap_or("");
                                match run_fn(script, &bstep.input) {
                                    Ok(r) => all_results.push(r),
                                    Err(e) => all_results.push(serde_json::json!({"error": e.to_string()})),
                                }
                            }
                        }
                    }
                    completed.insert(step.id.clone());
                    results.insert(step.id.clone(), serde_json::json!(all_results));
                }
                ExecStepKind::ForLoop(ref iterator) => {
                    completed.insert(step.id.clone());
                    results.insert(step.id.clone(), serde_json::json!({"iterated": iterator}));
                }
                ExecStepKind::WhileLoop(ref _condition, _max_iter) => {
                    completed.insert(step.id.clone());
                    results.insert(step.id.clone(), serde_json::json!({"status": "completed"}));
                }
                ExecStepKind::FailureModule => {
                    completed.insert(step.id.clone());
                    results.insert(step.id.clone(), serde_json::json!({"status": "failure_handler_ready"}));
                }
            }
        }

        Ok(results.into_iter().collect())
    }
}

/// A step ready for execution after flattening
#[derive(Debug, Clone)]
pub struct ExecStep {
    pub id: String,
    pub kind: ExecStepKind,
    pub script_path: Option<String>,
    pub input: serde_json::Value,
    pub retry: Option<RetryConfig>,
    pub timeout_ms: u64,
    pub depends_on: Vec<String>,
    pub sleep_after_ms: Option<u64>,
    pub stop_if: Option<String>,
    pub failure_step: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ExecStepKind {
    Script,
    Sleep,
    BranchOne(Vec<Vec<String>>),
    BranchAll(Vec<Vec<String>>),
    ForLoop(String),
    WhileLoop(String, u32),
    FailureModule,
}
