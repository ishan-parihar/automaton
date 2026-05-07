//! Flow DAG engine for Automaton.
//! Handles branch_one, branch_all, forloop, whileloop, failure_module execution.
//! When executed via the Runtime, modules are spawned as subprocesses with proper
//! state management and cross-step reference resolution.

use std::collections::HashMap;
use std::sync::Arc;

use automaton_core::backend::RegistryBackend;
use automaton_core::*;
use automaton_runtime::Runtime;
use chrono::Utc;

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
                FlowStepKind::Shell { command, shell } => {
                    result.push(ExecStep {
                        id: step.id.clone(),
                        kind: ExecStepKind::Shell { command: command.clone(), shell: shell.clone() },
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
                    let body_ids: Vec<String> = steps.iter().map(|s| s.id.clone()).collect();
                    result.push(ExecStep {
                        id: format!("{}__forloop", step.id),
                        kind: ExecStepKind::ForLoop {
                            iterator: iterator.clone(),
                            body_ids: body_ids.clone(),
                            body_steps: steps.clone(),
                        },
                        script_path: None,
                        input: step.input.clone(),
                        retry: None,
                        timeout_ms: step.timeout_ms.max(default_timeout),
                        depends_on: step.depends_on.clone(),
                        sleep_after_ms: None,
                        stop_if: None,
                        failure_step: None,
                    });
                    // Flatten body steps for dependency resolution
                    Self::flatten_steps(steps, default_retry, default_timeout, result, depth + 1)?;
                }
                FlowStepKind::WhileLoop { condition, steps, max_iterations } => {
                    let body_ids: Vec<String> = steps.iter().map(|s| s.id.clone()).collect();
                    result.push(ExecStep {
                        id: format!("{}__whileloop", step.id),
                        kind: ExecStepKind::WhileLoop {
                            condition: condition.clone(),
                            max_iterations: *max_iterations,
                            body_ids: body_ids.clone(),
                            body_steps: steps.clone(),
                        },
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
                FlowStepKind::CallFlow { flow_path, input } => {
                    result.push(ExecStep {
                        id: step.id.clone(),
                        kind: ExecStepKind::CallFlow { flow_path: flow_path.clone(), input: input.clone() },
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

    /// Execute a flow using the Runtime for actual module subprocess execution.
    /// Resolves `$var:/$res:` references via the Registry, evaluates loops,
    /// and manages cross-step flow state.
    /// This is the backward-compatible 4-param version (failure_handler = None).
    pub async fn execute(
        steps: &[ExecStep],
        backend: Option<&Arc<dyn RegistryBackend>>,
        runtime: &Runtime,
        build_cache_dir: &std::path::Path,
    ) -> Result<Vec<(String, serde_json::Value)>> {
        Self::execute_with_handlers(steps, backend, runtime, build_cache_dir, None).await
    }

    /// Like `execute()` but with an optional failure_handler module path.
    /// When a step fails and `failure_handler` is set (and the step has no
    /// per-step `failure_step`), a synthetic FailureModule step is recorded
    /// and execution continues instead of erroring out.
    ///
    /// This also executes all independent (dependency-met) steps concurrently
    /// via `futures::future::join_all`.
    pub async fn execute_with_handlers(
        steps: &[ExecStep],
        backend: Option<&Arc<dyn RegistryBackend>>,
        runtime: &Runtime,
        build_cache_dir: &std::path::Path,
        failure_handler: Option<&str>,
    ) -> Result<Vec<(String, serde_json::Value)>> {
        let mut results: HashMap<String, serde_json::Value> = HashMap::new();
        let mut completed = std::collections::HashSet::<String>::new();
        let mut pending: std::collections::VecDeque<usize> = (0..steps.len()).collect();
        let mut max_rounds = 1000;
        // Track completion order for deterministic output
        let mut completion_order: Vec<String> = Vec::new();

        while !pending.is_empty() && max_rounds > 0 {
            max_rounds -= 1;

            // Separate ready steps from not-ready steps
            let mut ready = Vec::new();
            let mut not_ready = Vec::new();
            while let Some(idx) = pending.pop_front() {
                if steps[idx].depends_on.iter().all(|d| completed.contains(d)) {
                    ready.push(idx);
                } else {
                    not_ready.push(idx);
                }
            }
            pending.extend(not_ready);

            if ready.is_empty() {
                // All remaining steps have unmet deps — deadlock
                break;
            }

            // Pre-resolve $var:/$res: references for all ready steps
            let mut resolved_inputs = Vec::with_capacity(ready.len());
            for &idx in &ready {
                let step = &steps[idx];
                let ri = if let Some(be) = &backend {
                    be.resolve_references(&step.input).await
                        .unwrap_or_else(|_| step.input.clone())
                } else {
                    step.input.clone()
                };
                resolved_inputs.push(ri);
            }

            // Build futures for all ready steps, handling stop_if inline
            let mut futs = Vec::with_capacity(ready.len());
            let mut skip_results: Vec<(String, serde_json::Value)> = Vec::new();

            for (&idx, ri) in ready.iter().zip(resolved_inputs.iter()) {
                let step = &steps[idx];

                // Check stop_if before dispatching
                let should_skip = step.stop_if.as_ref()
                    .map(|cond| evaluate_condition(cond, &results))
                    .unwrap_or(false);
                if should_skip {
                    let reason = step.stop_if.as_deref().unwrap_or("stop_if_triggered");
                    skip_results.push((
                        step.id.clone(),
                        serde_json::json!({"status": "skipped", "reason": reason}),
                    ));
                    continue;
                }

                // Resolve cross-step state references
                let resolved_input = crate::resolve_state_refs(ri, &results);

                // Clone data for the async future (avoid borrow conflicts)
                let step_clone = step.clone();
                let steps_vec = steps.to_vec();
                let bcd = build_cache_dir.to_path_buf();
                let be_clone = backend.cloned();
                let results_snapshot = results.clone();
                let step_id = step.id.clone();

                futs.push(async move {
                    let outcome = Self::execute_step_inner(
                        &step_clone,
                        &resolved_input,
                        &results_snapshot,
                        runtime,
                        &bcd,
                        be_clone.as_ref(),
                        &steps_vec,
                    ).await;
                    (step_id, outcome)
                });
            }

            // Process skipped results immediately
            for (id, val) in skip_results {
                completed.insert(id.clone());
                completion_order.push(id.clone());
                results.insert(id, val);
            }

            // Execute all ready steps concurrently
            let outcomes: Vec<(String, std::result::Result<serde_json::Value, AutomatonError>)> =
                futures::future::join_all(futs).await;

            for (step_id, outcome) in outcomes {
                match outcome {
                    Ok(output) => {
                        completed.insert(step_id.clone());
                        completion_order.push(step_id.clone());
                        results.insert(step_id, output);
                    }
                    Err(e) => {
                        // Check per-step failure_step first
                        let step_info = steps.iter().find(|s| s.id == step_id).ok_or_else(||
                            AutomatonError::Other(format!("Step '{step_id}' not found after execution"))
                        )?;
                        if let Some(fallback) = &step_info.failure_step {
                            completed.insert(step_id.clone());
                            completion_order.push(step_id.clone());
                            results.insert(step_id, serde_json::json!({
                                "error": e.to_string(),
                                "fallback": fallback,
                            }));
                        } else if let Some(handler) = failure_handler {
                            // Use the on_failure handler — record a synthetic FailureModule
                            let handler_id = "__on_failure__";
                            completed.insert(step_id.clone());
                            completion_order.push(step_id.clone());
                            results.insert(step_id.clone(), serde_json::json!({
                                "error": e.to_string(),
                                "handler_triggered": handler,
                            }));
                            completed.insert(handler_id.to_string());
                            completion_order.push(handler_id.to_string());
                            results.insert(handler_id.to_string(), serde_json::json!({
                                "status": "failure_handler_ready",
                                "handled_error": e.to_string(),
                                "for_step": step_id,
                                "handler": handler,
                            }));
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
        }

        // Return results in completion order (HashMap has undefined iteration order)
        let ordered: Vec<(String, serde_json::Value)> = completion_order.iter()
            .filter_map(|id| results.remove(id).map(|v| (id.clone(), v)))
            .collect();
        Ok(ordered)
    }

    /// Execute a flow with the same semantics as `execute_with_handlers` but
    /// collects per-step telemetry (timing, status, output/error) and returns
    /// it alongside the results. An optional `progress_callback` is invoked
    /// after each step completes with (completed_count, total_count, &StepTelemetry).
    pub async fn execute_with_telemetry(
        steps: &[ExecStep],
        backend: Option<&Arc<dyn RegistryBackend>>,
        runtime: &Runtime,
        build_cache_dir: &std::path::Path,
        failure_handler: Option<&str>,
        progress_callback: Option<Box<dyn Fn(usize, usize, &StepTelemetry) + Send + Sync>>,
    ) -> Result<(Vec<(String, serde_json::Value)>, Vec<StepTelemetry>)> {
        let mut results: HashMap<String, serde_json::Value> = HashMap::new();
        let mut completed = std::collections::HashSet::<String>::new();
        let mut pending: std::collections::VecDeque<usize> = (0..steps.len()).collect();
        let mut max_rounds = 1000;
        let mut completion_order: Vec<String> = Vec::new();
        let mut telemetry: Vec<StepTelemetry> = Vec::new();
        let total_steps = steps.len();

        while !pending.is_empty() && max_rounds > 0 {
            max_rounds -= 1;

            // Separate ready steps from not-ready steps
            let mut ready = Vec::new();
            let mut not_ready = Vec::new();
            while let Some(idx) = pending.pop_front() {
                if steps[idx].depends_on.iter().all(|d| completed.contains(d)) {
                    ready.push(idx);
                } else {
                    not_ready.push(idx);
                }
            }
            pending.extend(not_ready);

            if ready.is_empty() {
                break;
            }

            // Pre-resolve $var:/$res: references for all ready steps
            let mut resolved_inputs = Vec::with_capacity(ready.len());
            for &idx in &ready {
                let step = &steps[idx];
                let ri = if let Some(be) = &backend {
                    be.resolve_references(&step.input).await
                        .unwrap_or_else(|_| step.input.clone())
                } else {
                    step.input.clone()
                };
                resolved_inputs.push(ri);
            }

            // Build futures for all ready steps, handling stop_if inline
            let mut futs = Vec::with_capacity(ready.len());
            let mut skip_results: Vec<(String, serde_json::Value, StepTelemetry)> = Vec::new();

            for (&idx, ri) in ready.iter().zip(resolved_inputs.iter()) {
                let step = &steps[idx];

                // Check stop_if before dispatching
                let should_skip = step.stop_if.as_ref()
                    .map(|cond| evaluate_condition(cond, &results))
                    .unwrap_or(false);
                if should_skip {
                    let reason = step.stop_if.as_deref().unwrap_or("stop_if_triggered");
                    let t = StepTelemetry {
                        step_id: step.id.clone(),
                        step_kind: step_kind_name(&step.kind),
                        status: StepStatus::Skipped(reason.to_string()),
                        started_at: None,
                        completed_at: None,
                        duration_ms: Some(0),
                        retry_attempt: 0,
                        output: Some(serde_json::json!({"status": "skipped", "reason": reason})),
                        error: None,
                    };
                    skip_results.push((
                        step.id.clone(),
                        serde_json::json!({"status": "skipped", "reason": reason}),
                        t,
                    ));
                    continue;
                }

                // Resolve cross-step state references
                let resolved_input = crate::resolve_state_refs(ri, &results);

                // Clone data for the async future (avoid borrow conflicts)
                let step_clone = step.clone();
                let steps_vec = steps.to_vec();
                let bcd = build_cache_dir.to_path_buf();
                let be_clone = backend.cloned();
                let results_snapshot = results.clone();
                let step_id = step.id.clone();

                futs.push(async move {
                    let started_at = Utc::now();
                    let start_instant = std::time::Instant::now();
                    let outcome = Self::execute_step_inner(
                        &step_clone,
                        &resolved_input,
                        &results_snapshot,
                        runtime,
                        &bcd,
                        be_clone.as_ref(),
                        &steps_vec,
                    ).await;
                    let elapsed_ms = start_instant.elapsed().as_millis() as u64;
                    let completed_at = Utc::now();

                    let telemetry_data = match &outcome {
                        Ok(val) => StepTelemetry {
                            step_id: step_id.clone(),
                            step_kind: step_kind_name(&step_clone.kind),
                            status: StepStatus::Completed,
                            started_at: Some(started_at),
                            completed_at: Some(completed_at),
                            duration_ms: Some(elapsed_ms),
                            retry_attempt: 0,
                            output: Some(val.clone()),
                            error: None,
                        },
                        Err(e) => StepTelemetry {
                            step_id: step_id.clone(),
                            step_kind: step_kind_name(&step_clone.kind),
                            status: StepStatus::Failed(e.to_string()),
                            started_at: Some(started_at),
                            completed_at: Some(completed_at),
                            duration_ms: Some(elapsed_ms),
                            retry_attempt: 0,
                            output: None,
                            error: Some(e.to_string()),
                        },
                    };

                    (step_id, outcome, telemetry_data)
                });
            }

            // Process skipped results immediately
            for (id, val, t) in skip_results {
                completed.insert(id.clone());
                completion_order.push(id.clone());
                results.insert(id, val);
                // Notify progress AFTER recording completion
                if let Some(ref cb) = progress_callback {
                    cb(completion_order.len(), total_steps, &t);
                }
                telemetry.push(t);
            }

            // Execute all ready steps concurrently
            let outcomes: Vec<(String, std::result::Result<serde_json::Value, AutomatonError>, StepTelemetry)> =
                futures::future::join_all(futs).await;

            for (step_id, outcome, t) in outcomes {
                match outcome {
                    Ok(output) => {
                        completed.insert(step_id.clone());
                        completion_order.push(step_id.clone());
                        results.insert(step_id, output);
                        if let Some(ref cb) = progress_callback {
                            cb(completion_order.len(), total_steps, &t);
                        }
                        telemetry.push(t);
                    }
                    Err(e) => {
                        // Check per-step failure_step first
                        let step_info = steps.iter().find(|s| s.id == step_id).ok_or_else(||
                            AutomatonError::Other(format!("Step '{step_id}' not found after execution"))
                        )?;
                        if let Some(fallback) = &step_info.failure_step {
                            completed.insert(step_id.clone());
                            completion_order.push(step_id.clone());
                            results.insert(step_id, serde_json::json!({
                                "error": e.to_string(),
                                "fallback": fallback,
                            }));
                            if let Some(ref cb) = progress_callback {
                                cb(completion_order.len(), total_steps, &t);
                            }
                            telemetry.push(t);
                        } else if let Some(handler) = failure_handler {
                            // Use the on_failure handler — record a synthetic FailureModule
                            let handler_id = "__on_failure__";
                            completed.insert(step_id.clone());
                            completion_order.push(step_id.clone());
                            results.insert(step_id.clone(), serde_json::json!({
                                "error": e.to_string(),
                                "handler_triggered": handler,
                            }));
                            completed.insert(handler_id.to_string());
                            completion_order.push(handler_id.to_string());
                            results.insert(handler_id.to_string(), serde_json::json!({
                                "status": "failure_handler_ready",
                                "handled_error": e.to_string(),
                                "for_step": step_id,
                                "handler": handler,
                            }));
                            if let Some(ref cb) = progress_callback {
                                cb(completion_order.len(), total_steps, &t);
                            }
                            telemetry.push(t);
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
        }

        // Return results in completion order
        let ordered: Vec<(String, serde_json::Value)> = completion_order.iter()
            .filter_map(|id| results.remove(id).map(|v| (id.clone(), v)))
            .collect();
        Ok((ordered, telemetry))
    }

    /// Execute a SINGLE step with the given resolved input and state snapshot.
    /// Returns the output value for this step.
    async fn execute_step_inner(
        step: &ExecStep,
        resolved_input: &serde_json::Value,
        results_snapshot: &HashMap<String, serde_json::Value>,
        runtime: &Runtime,
        build_cache_dir: &std::path::Path,
        backend: Option<&Arc<dyn RegistryBackend>>,
        steps: &[ExecStep],
    ) -> Result<serde_json::Value> {
        match &step.kind {
            ExecStepKind::Script => {
                let script_path = step.script_path.as_deref().unwrap_or("");
                let binary_path = build_cache_dir.join(script_path.replace('.', "_"));
                if binary_path.exists() {
                    if let Some(retry_cfg) = &step.retry {
                        runtime.run_with_retry(&binary_path, resolved_input, retry_cfg, step.timeout_ms).await
                    } else {
                        runtime.run_binary(&binary_path, resolved_input, step.timeout_ms).await
                    }
                } else {
                    Ok(serde_json::json!({"status": "no_binary_found", "path": script_path}))
                }
            }
            ExecStepKind::Shell { command, shell } => {
                let shell_bin = shell.as_deref().unwrap_or("sh");

                // Shell execution helper with kill_on_drop for orphan cleanup
                let run_shell = || async {
                    let timeout = std::time::Duration::from_millis(step.timeout_ms);
                    let child = tokio::process::Command::new(shell_bin)
                        .arg("-c")
                        .arg(command.as_str())
                        .kill_on_drop(true)
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn()
                        .map_err(|e| AutomatonError::ExecutionFailed(format!("Failed to spawn shell: {e}")))?;

                    match tokio::time::timeout(timeout, child.wait_with_output()).await {
                        Ok(Ok(output)) => {
                            if output.status.success() {
                                Ok(serde_json::json!({
                                    "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                                    "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                                    "exit_code": output.status.code().unwrap_or(0),
                                }))
                            } else {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                Err(AutomatonError::ExecutionFailed(format!(
                                    "Shell command failed (exit {}): {}",
                                    output.status.code().unwrap_or(-1),
                                    if stderr.is_empty() { &stdout } else { &stderr },
                                )))
                            }
                        }
                        Ok(Err(e)) => Err(AutomatonError::ExecutionFailed(format!("Shell process error: {e}"))),
                        Err(_) => Err(AutomatonError::Timeout(step.timeout_ms)),
                    }
                };

                // Run with optional retry
                if let Some(retry_cfg) = &step.retry {
                    let mut last_error = String::new();
                    let mut delay = retry_cfg.delay_ms;
                    let mut output = None;
                    for attempt in 1..=retry_cfg.max_attempts {
                        match run_shell().await {
                            Ok(out) => { output = Some(out); break; }
                            Err(e) => {
                                last_error = e.to_string();
                                if attempt < retry_cfg.max_attempts {
                                    if delay > 0 {
                                        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                                    }
                                    delay = match retry_cfg.backoff {
                                        automaton_core::BackoffKind::Fixed => retry_cfg.delay_ms,
                                        automaton_core::BackoffKind::Linear => retry_cfg.delay_ms * (attempt as u64 + 1),
                                        automaton_core::BackoffKind::Exponential => retry_cfg.delay_ms * (1u64 << attempt),
                                    };
                                }
                            }
                        }
                    }
                    output.ok_or_else(|| AutomatonError::ExecutionFailed(
                        format!("All {} attempts failed. Last error: {last_error}", retry_cfg.max_attempts)
                    ))
                } else {
                    run_shell().await
                }
            }
            ExecStepKind::Sleep => {
                let ms = step.sleep_after_ms.unwrap_or(1000);
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                Ok(serde_json::json!({"slept_ms": ms}))
            }
            ExecStepKind::BranchOne(branches) => {
                    let mut branch_result = None;
                for branch_ids in branches {
                    for bid in branch_ids {
                        if let Some(bstep) = steps.iter().find(|s| s.id == *bid) {
                            let script_path = bstep.script_path.as_deref().unwrap_or("");
                            let bp = build_cache_dir.join(script_path.replace('.', "_"));
                            if bp.exists() {
                                match runtime.run_binary(&bp, resolved_input, bstep.timeout_ms).await {
                                    Ok(r) => { branch_result = Some(r); break; }
                                    Err(_) => continue,
                                }
                            } else if let ExecStepKind::Shell { command, shell } = &bstep.kind {
                                // Shell-based fallback: first branch that succeeds
                                let shell_bin = shell.as_deref().unwrap_or("sh");
                                let child = tokio::process::Command::new(shell_bin)
                                    .arg("-c")
                                    .arg(command.as_str())
                                    .kill_on_drop(true)
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn()
                                    .map_err(|e| AutomatonError::ExecutionFailed(format!("BranchOne shell: {e}")))?;
                                let to = std::time::Duration::from_millis(bstep.timeout_ms.max(step.timeout_ms));
                                match tokio::time::timeout(to, child.wait_with_output()).await {
                                    Ok(Ok(output)) if output.status.success() => {
                                        branch_result = Some(serde_json::json!({
                                            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                                            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                                            "exit_code": output.status.code().unwrap_or(0),
                                        }));
                                        break;
                                    }
                                    _ => continue,
                                }
                            }
                        }
                    }
                    if branch_result.is_some() { break; }
                }
                Ok(branch_result.unwrap_or(serde_json::json!({"error": "all_branches_failed"})))
            }
            ExecStepKind::BranchAll(branches) => {
                let mut all_results = Vec::new();
                for branch_ids in branches {
                    for bid in branch_ids {
                        if let Some(bstep) = steps.iter().find(|s| s.id == *bid) {
                            let script_path = bstep.script_path.as_deref().unwrap_or("");
                            let bp = build_cache_dir.join(script_path.replace('.', "_"));
                            if bp.exists() {
                                match runtime.run_binary(&bp, resolved_input, bstep.timeout_ms).await {
                                    Ok(r) => all_results.push(r),
                                    Err(e) => all_results.push(serde_json::json!({"error": e.to_string()})),
                                }
                            } else if let ExecStepKind::Shell { command, shell } = &bstep.kind {
                                // Shell-based fallback for all branches
                                let shell_bin = shell.as_deref().unwrap_or("sh");
                                let child = tokio::process::Command::new(shell_bin)
                                    .arg("-c")
                                    .arg(command.as_str())
                                    .kill_on_drop(true)
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn()
                                    .map_err(|e| AutomatonError::ExecutionFailed(format!("BranchAll shell: {e}")))?;
                                let to = std::time::Duration::from_millis(bstep.timeout_ms.max(step.timeout_ms));
                                match tokio::time::timeout(to, child.wait_with_output()).await {
                                    Ok(Ok(output)) => {
                                        all_results.push(serde_json::json!({
                                            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                                            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                                            "exit_code": output.status.code().unwrap_or(0),
                                        }));
                                    }
                                    Ok(Err(e)) => all_results.push(serde_json::json!({"error": e.to_string()})),
                                    Err(_) => all_results.push(serde_json::json!({"error": "timeout"})),
                                }
                            }
                        }
                    }
                }
                Ok(serde_json::json!(all_results))
            }
            ExecStepKind::ForLoop { iterator, body_ids, body_steps } => {
                let mut local_results = results_snapshot.clone();
                let iterable = resolve_iterable(resolved_input, results_snapshot.get(iterator));
                let mut loop_results = Vec::new();
                for (i, item) in iterable.iter().enumerate() {
                    local_results.insert(format!("{}__item", iterator), item.clone());
                    local_results.insert(format!("{}__index", iterator), serde_json::json!(i));
                    for body_id in body_ids {
                        if let Some(body_step) = body_steps.iter().find(|s| s.id == *body_id) {
                            let script_path = body_step.script_path.as_deref().unwrap_or("");
                            let bp = build_cache_dir.join(script_path.replace('.', "_"));
                            if bp.exists() {
                                let body_input = crate::resolve_state_refs(&body_step.input, &local_results);
                                match runtime.run_binary(&bp, &body_input, body_step.timeout_ms).await {
                                    Ok(r) => { local_results.insert(body_id.clone(), r.clone()); loop_results.push(r); }
                                    Err(e) => { loop_results.push(serde_json::json!({"error": e.to_string()})); }
                                }
                            } else if let FlowStepKind::Shell { command, shell } = &body_step.kind {
                                // Shell-based fallback for loop body
                                let shell_bin = shell.as_deref().unwrap_or("sh");
                                let resolved_cmd = crate::resolve_state_refs(
                                    &serde_json::json!(command.as_str()), &local_results,
                                );
                                let cmd_str = resolved_cmd.as_str().unwrap_or(command);
                                let child = tokio::process::Command::new(shell_bin)
                                    .arg("-c")
                                    .arg(cmd_str)
                                    .kill_on_drop(true)
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn()
                                    .map_err(|e| AutomatonError::ExecutionFailed(format!("ForLoop shell: {e}")))?;
                                let to = std::time::Duration::from_millis(body_step.timeout_ms.max(step.timeout_ms));
                                match tokio::time::timeout(to, child.wait_with_output()).await {
                                    Ok(Ok(output)) if output.status.success() => {
                                        let val = serde_json::json!({
                                            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                                            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                                            "exit_code": output.status.code().unwrap_or(0),
                                        });
                                        local_results.insert(body_id.clone(), val.clone());
                                        loop_results.push(val);
                                    }
                                    Ok(Ok(output)) => {
                                        let err = format!("exit {}", output.status.code().unwrap_or(-1));
                                        loop_results.push(serde_json::json!({"error": err}));
                                    }
                                    Ok(Err(e)) => loop_results.push(serde_json::json!({"error": e.to_string()})),
                                    Err(_) => loop_results.push(serde_json::json!({"error": "timeout"})),
                                }
                            }
                        }
                    }
                }
                Ok(serde_json::json!({
                    "iterations": iterable.len(),
                    "results": loop_results,
                }))
            }
            ExecStepKind::WhileLoop { condition, max_iterations, body_ids, body_steps } => {
                let mut local_results = results_snapshot.clone();
                let mut while_results = Vec::new();
                let mut iteration = 0usize;
                while evaluate_condition(condition, &local_results) && iteration < *max_iterations as usize {
                    for body_id in body_ids {
                        if let Some(body_step) = body_steps.iter().find(|s| s.id == *body_id) {
                            let script_path = body_step.script_path.as_deref().unwrap_or("");
                            let bp = build_cache_dir.join(script_path.replace('.', "_"));
                            if bp.exists() {
                                let body_input = crate::resolve_state_refs(&body_step.input, &local_results);
                                match runtime.run_binary(&bp, &body_input, body_step.timeout_ms).await {
                                    Ok(r) => { local_results.insert(body_id.clone(), r.clone()); while_results.push(r); }
                                    Err(e) => { while_results.push(serde_json::json!({"error": e.to_string()})); }
                                }
                            } else if let FlowStepKind::Shell { command, shell } = &body_step.kind {
                                // Shell-based fallback for while body
                                let shell_bin = shell.as_deref().unwrap_or("sh");
                                let resolved_cmd = crate::resolve_state_refs(
                                    &serde_json::json!(command.as_str()), &local_results,
                                );
                                let cmd_str = resolved_cmd.as_str().unwrap_or(command);
                                let child = tokio::process::Command::new(shell_bin)
                                    .arg("-c")
                                    .arg(cmd_str)
                                    .kill_on_drop(true)
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn()
                                    .map_err(|e| AutomatonError::ExecutionFailed(format!("WhileLoop shell: {e}")))?;
                                let to = std::time::Duration::from_millis(body_step.timeout_ms.max(step.timeout_ms));
                                match tokio::time::timeout(to, child.wait_with_output()).await {
                                    Ok(Ok(output)) if output.status.success() => {
                                        let val = serde_json::json!({
                                            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                                            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                                            "exit_code": output.status.code().unwrap_or(0),
                                        });
                                        local_results.insert(body_id.clone(), val.clone());
                                        while_results.push(val);
                                    }
                                    Ok(Ok(output)) => {
                                        let err = format!("exit {}", output.status.code().unwrap_or(-1));
                                        while_results.push(serde_json::json!({"error": err}));
                                    }
                                    Ok(Err(e)) => while_results.push(serde_json::json!({"error": e.to_string()})),
                                    Err(_) => while_results.push(serde_json::json!({"error": "timeout"})),
                                }
                            }
                        }
                    }
                    iteration += 1;
                }
                Ok(serde_json::json!({
                    "iterations": iteration,
                    "results": while_results,
                }))
            }
            ExecStepKind::FailureModule => {
                Ok(serde_json::json!({"status": "failure_handler_ready"}))
            }
            ExecStepKind::CallFlow { flow_path, .. } => {
                let be = backend.ok_or_else(|| AutomatonError::Other(
                    format!("CallFlow '{flow_path}': no backend available for flow lookup")
                ))?;
                let flow_val = be.get_flow(flow_path).await?
                    .ok_or_else(|| AutomatonError::Other(
                        format!("CallFlow '{flow_path}': flow not found")
                    ))?;
                let def: FlowDefinition = serde_json::from_value(
                    flow_val.get("definition")
                        .cloned()
                        .ok_or_else(|| AutomatonError::Other(
                            format!("CallFlow '{flow_path}': missing 'definition' field")
                        ))?
                )?;
                let child_steps = Self::flatten(&def)?;
                let child_results = Self::execute_with_handlers(
                    &child_steps,
                    backend,
                    runtime,
                    build_cache_dir,
                    None,
                ).await?;
                let merged: serde_json::Map<String, serde_json::Value> = child_results.into_iter().collect();
                Ok(serde_json::Value::Object(merged))
            }
        }
    }
}

/// Resolve an iterator value from a ForLoop definition.
/// Checks the resolved_input for an array, then falls back to upstream flow state.
fn resolve_iterable(resolved_input: &serde_json::Value, upstream: Option<&serde_json::Value>) -> Vec<serde_json::Value> {
    // First check the flow input
    if let Some(arr) = resolved_input.as_array() {
        return arr.clone();
    }
    // Then check upstream state
    if let Some(up) = upstream {
        if let Some(arr) = up.as_array() {
            return arr.clone();
        }
        // If it's an object with an 'items' or 'results' field
        if let Some(items) = up.get("items").and_then(|v| v.as_array()) {
            return items.clone();
        }
        if let Some(results) = up.get("results").and_then(|v| v.as_array()) {
            return results.clone();
        }
        // Wrap scalar in a single-element vec
        if !up.is_null() {
            return vec![up.clone()];
        }
    }
    // Default: single iteration with empty object
    vec![serde_json::json!({})]
}

/// Evaluate a WhileLoop condition expression against current flow state.
/// Supports simple expressions like `${step.status} == "completed"`.
fn evaluate_condition(condition: &str, results: &HashMap<String, serde_json::Value>) -> bool {
    // Simple evaluation: resolve state refs then check equality
    let cond = condition.trim();
    if cond.is_empty() {
        return false;
    }

    // If the condition is just a state reference like `${step.status}`,
    // check if it resolves to a truthy value
    if cond.starts_with("${") && cond.ends_with("}") && !cond.contains(' ') {
        let ref_val = crate::resolve_state_refs(&serde_json::json!(cond), results);
        return ref_val.is_object() || ref_val.is_array() || ref_val.as_bool() == Some(true)
            || ref_val.as_str().map(|s| s == "true" || s == "completed" || s == "ok").unwrap_or(false)
            || (ref_val.is_number() && ref_val.as_f64().map(|v| v > 0.0).unwrap_or(false));
    }

    // Simple string comparison: "${x.status} == completed"
    if let Some(eq_pos) = cond.find("==") {
        let left_str = cond[..eq_pos].trim();
        let right_str = cond[eq_pos + 2..].trim().trim_matches('"');
        let left_val = if left_str.starts_with("${") {
            crate::resolve_state_refs(&serde_json::json!(left_str), results)
        } else {
            serde_json::json!(left_str)
        };
        let left_display = left_val.as_str().map(|s| s.to_string())
            .or_else(|| left_val.as_f64().map(|n| n.to_string()))
            .unwrap_or_default();
        return left_display == right_str;
    }

    // Simple string inequality: "${x.count} > 0"
    if let Some(gt_pos) = cond.find('>') {
        let left_str = cond[..gt_pos].trim();
        let right_str = cond[gt_pos + 1..].trim();
        let left_val = if left_str.starts_with("${") {
            crate::resolve_state_refs(&serde_json::json!(left_str), results)
        } else {
            serde_json::json!(left_str)
        };
        let left_num = left_val.as_f64().unwrap_or(0.0);
        let right_num: f64 = right_str.parse().unwrap_or(0.0);
        return left_num > right_num;
    }

    // Default: resolve the condition as a state ref and check truthiness
    let resolved = crate::resolve_state_refs(&serde_json::json!(cond), results);
    resolved.as_bool() == Some(true)
        || resolved.as_str().map(|s| s == "true" || s == "1").unwrap_or(false)
        || resolved.as_f64().map(|v| v > 0.0).unwrap_or(false)
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
    Shell {
        command: String,
        shell: Option<String>,
    },
    Sleep,
    BranchOne(Vec<Vec<String>>),
    BranchAll(Vec<Vec<String>>),
    ForLoop {
        iterator: String,
        body_ids: Vec<String>,
        body_steps: Vec<FlowStep>,
    },
    WhileLoop {
        condition: String,
        max_iterations: u32,
        body_ids: Vec<String>,
        body_steps: Vec<FlowStep>,
    },
    FailureModule,
    CallFlow {
        flow_path: String,
        input: Option<serde_json::Value>,
    },
}

/// Convert an `ExecStepKind` into a short human-readable string for telemetry.
fn step_kind_name(kind: &ExecStepKind) -> String {
    match kind {
        ExecStepKind::Script => "script".into(),
        ExecStepKind::Shell { .. } => "shell".into(),
        ExecStepKind::Sleep => "sleep".into(),
        ExecStepKind::BranchOne(_) => "branch_one".into(),
        ExecStepKind::BranchAll(_) => "branch_all".into(),
        ExecStepKind::ForLoop { .. } => "for_loop".into(),
        ExecStepKind::WhileLoop { .. } => "while_loop".into(),
        ExecStepKind::FailureModule => "failure_module".into(),
        ExecStepKind::CallFlow { .. } => "call_flow".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_condition_truthy_state_ref() {
        let mut results = HashMap::new();
        results.insert("step_a".into(), serde_json::json!({"status": "completed"}));
        assert!(evaluate_condition("${step_a.status}", &results));
    }

    #[test]
    fn test_evaluate_condition_eq_comparison() {
        let mut results = HashMap::new();
        results.insert("step_a".into(), serde_json::json!({"status": "completed"}));
        assert!(evaluate_condition(r#"${step_a.status} == "completed""#, &results));
        assert!(!evaluate_condition(r#"${step_a.status} == "failed""#, &results));
    }

    #[test]
    fn test_evaluate_condition_gt_comparison() {
        let mut results = HashMap::new();
        results.insert("step_a".into(), serde_json::json!({"count": 5}));
        assert!(evaluate_condition("${step_a.count} > 0", &results));
        assert!(!evaluate_condition("${step_a.count} > 10", &results));
    }

    #[test]
    fn test_evaluate_condition_empty() {
        assert!(!evaluate_condition("", &HashMap::new()));
    }

    #[test]
    fn test_resolve_iterable_from_input() {
        let input = serde_json::json!([1, 2, 3]);
        let result = resolve_iterable(&input, None);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_iterable_from_upstream() {
        let upstream = serde_json::json!([4, 5, 6]);
        let result = resolve_iterable(&serde_json::json!({}), Some(&upstream));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_iterable_from_results_field() {
        let upstream = serde_json::json!({"results": ["a", "b"]});
        let result = resolve_iterable(&serde_json::json!({}), Some(&upstream));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_resolve_iterable_default() {
        let result = resolve_iterable(&serde_json::json!({}), None);
        assert_eq!(result.len(), 1);
    }
}
