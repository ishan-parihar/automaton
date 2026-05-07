# Flow Composition Guide

## 1. Overview

A flow is an ordered set of steps arranged as a dependency DAG. Each step
declares which other steps it depends on. The engine resolves these
dependencies to determine execution order. Steps with no dependencies between
them run in parallel.

Flows differ from engine DAGs in purpose and structure. Engine DAGs are
petgraph-based parallel execution graphs materialized at runtime from the
design graph. Flows are explicit step lists with dependency ordering for
sequential and conditional execution. You write flows when you want a
repeatable pipeline with branches, loops, sub-flow calls, and shell
commands alongside compiled WASM modules.

A flow definition is JSON stored in the registry. It has a path, version,
step list, default configuration, and optional failure handler.

## 2. Step Kinds

Every flow step has a kind that determines what the engine does when it
reaches that step.

| Kind | Fields | Description |
|------|--------|-------------|
| Script | (none) | Run a compiled WASM module by path. The `script_path` field points to a module path in the registry. The engine looks for a compiled binary in the build cache. |
| Shell | `command`, `shell?` | Execute a shell command. Uses `sh -c` by default (customize via `shell`). Kills orphan processes on drop. Returns `{stdout, stderr, exit_code}`. |
| Sleep | (none) | Pause for N milliseconds. Duration comes from `sleep_after_ms`, defaults to 1000ms. |
| BranchOne | `[steps]` | Try each branch sequentially. Return the result of the first success. Remaining branches are skipped. |
| BranchAll | `[steps]` | Run all branches. Results merge into an array. Each branch runs regardless of others success or failure. |
| ForLoop | `iterator`, `steps` | Iterate over a resolved iterable. For each item, run the body steps. The current item and index are injected into the step context. |
| WhileLoop | `condition`, `steps`, `max_iterations` | Loop while `condition` evaluates to true. The condition string supports state reference expressions. `max_iterations` is a hard cap. |
| CallFlow | `flow_path`, `input?` | Execute another flow from the registry. The sub-flow is deserialized, flattened, and executed recursively. Its results merge into the parent context. |
| FailureModule | (none) | A synthetic placeholder for on_failure handler execution. Created automatically by the engine when a step fails and the flow has an `on_failure` configured. |

Common fields on every step:

- **id** (string): Unique identifier within the flow
- **script_path** (string, optional): Module path for Script steps
- **input** (object, optional): Input data passed to the step
- **retry** (RetryConfig, optional): Per-step retry override
- **timeout_ms** (u64): Execution timeout in milliseconds
- **depends_on** (string[]): Step IDs this step depends on
- **sleep_after_ms** (u64, optional): Sleep after step completion
- **stop_if** (string, optional): Condition that when true skips this step
- **failure_step** (string, optional): Step ID for error reporting instead of aborting

## 3. Dependencies

Steps declare ordering constraints with `depends_on`. A step listed in
`depends_on` must complete before this step starts. Steps with no
dependencies are root steps and become ready immediately.

```json
[
  { "id": "fetch_data", "kind": "Script", "script_path": "data.fetcher" },
  { "id": "validate", "kind": "Script", "script_path": "data.validator",
    "depends_on": ["fetch_data"] },
  { "id": "transform", "kind": "Script", "script_path": "data.transformer",
    "depends_on": ["fetch_data"] },
  { "id": "report", "kind": "Shell", "command": "cat results.json",
    "depends_on": ["validate", "transform"] }
]
```

`fetch_data` runs first. `validate` and `transform` both depend on
`fetch_data` but not on each other, so they run in parallel once
`fetch_data` completes. `report` depends on both, so it waits until both
finish.

## 4. Shell Execution

Shell steps spawn a subprocess via `tokio::process::Command`:

- Shell binary defaults to `sh`, overridable via the `shell` field
- Command is passed as `-c <command>`
- `kill_on_drop(true)` cleans up orphans on timeout or cancellation
- Stdout and stderr are piped and captured

**Output format:**

```json
{
  "stdout": "Hello from the command\n",
  "stderr": "",
  "exit_code": 0
}
```

**Timeout:** The engine wraps the process in `tokio::time::timeout`. If the
process does not complete within `timeout_ms`, the step fails and the process
is dropped (triggering kill_on_drop).

**Retry:** Shell and Script steps can be configured with retry logic:

```json
{
  "id": "flaky_network_call",
  "kind": "Shell",
  "command": "curl https://api.example.com/data",
  "retry": { "max_attempts": 5, "delay_ms": 2000, "backoff": "exponential" },
  "timeout_ms": 10000
}
```

Three backoff strategies:

- **Fixed**: Same delay between every attempt (`delay_ms`)
- **Linear**: Delay grows as `delay_ms * (attempt + 1)`
- **Exponential**: Delay doubles as `delay_ms * (1 << attempt)`

**stop_if:** A step can declare a condition that, when true, causes the step
to be skipped without error:

```json
{
  "id": "optional_step", "kind": "Shell", "command": "do_something",
  "stop_if": "${prev_step.status} == \"skipped\""
}
```

**failure_step:** When a step fails, instead of halting, the engine records
the error and continues. The `failure_step` value is a step ID that receives
error context:

```json
{
  "id": "risky_operation", "kind": "Shell", "command": "dangerous_command",
  "failure_step": "error_logger"
}
```

## 5. Branch and Loop

### BranchOne

BranchOne tries each branch in order and returns the first success.
Remaining branches are skipped. Useful for fallback patterns.

```json
{
  "id": "try_providers", "kind": "BranchOne",
  "branches": [
    [{ "id": "try_primary", "kind": "Shell", "command": "curl primary.api" }],
    [{ "id": "try_secondary", "kind": "Shell", "command": "curl secondary.api" }],
    [{ "id": "try_cache", "kind": "Shell", "command": "cat cached_data.json" }]
  ]
}
```

If primary succeeds, secondary and cache are never tried. If all fail, the
step returns `{"error": "all_branches_failed"}`. When no compiled WASM binary
exists for a body step, the engine falls back to Shell execution.

### BranchAll

BranchAll runs every branch independently and merges results into an array.
Each branches result is included regardless of success or failure.

```json
{
  "id": "collect_metrics", "kind": "BranchAll",
  "branches": [
    [{ "id": "cpu", "kind": "Shell", "command": "get_cpu_usage" }],
    [{ "id": "memory", "kind": "Shell", "command": "get_memory_usage" }],
    [{ "id": "disk", "kind": "Shell", "command": "get_disk_usage" }]
  ]
}
```

### ForLoop

ForLoop iterates over a resolved iterable. The engine finds the iterable by
checking the step input (for an array), then falling back to upstream flow
state (checking for the iterator key, then `items`, then `results`, then
wrapping a scalar in a single-element array).

```json
{
  "id": "process_items", "kind": "ForLoop", "iterator": "items",
  "steps": [
    { "id": "process_one", "kind": "Shell",
      "command": "process_item ${items__item}" }
  ]
}
```

During each iteration, the engine injects `${items__item}` (the current item)
and `${items__index}` (the 0-based index) into local state.

### WhileLoop

WhileLoop evaluates a condition string before each iteration. The condition
supports state reference expressions:

- `${step_id.field}` resolves to a value from a completed step
- `== "value"` does string comparison
- `> N` does numeric comparison
- Bare state references check truthiness (non-null, non-zero, truthy string)

```json
{
  "id": "retry_until_done", "kind": "WhileLoop",
  "condition": "${worker.status} != \"completed\"", "max_iterations": 10,
  "steps": [
    { "id": "worker", "kind": "Shell", "command": "check_and_process" }
  ]
}
```

Body steps follow the same fallback as ForLoop: when no WASM binary exists,
the engine falls back to Shell execution.

## 6. Flow Composition (CallFlow)

CallFlow enables hierarchical composition. One flow executes another from
the registry, and the sub-flows results merge into the parent context.

```json
{
  "id": "process_user", "kind": "CallFlow",
  "flow_path": "users.process",
  "input": { "user_id": "${fetch_user.id}" }
}
```

When the engine encounters CallFlow, it:

1. Looks up the flow by `flow_path` via the registry backend
2. Deserializes the stored `FlowDefinition`
3. Flattens the sub-flow into executable steps
4. Recursively executes the sub-flow via `execute_with_handlers`
5. Merges all step results from the sub-flow into the parent context

This enables reusable flow libraries. A top-level pipeline can orchestrate
dozens of sub-flows, each independently versioned and tested.

```json
{
  "id": "notify", "kind": "CallFlow", "depends_on": ["process_user"],
  "flow_path": "notifications.email",
  "input": { "to": "${process_user.email}", "template": "welcome" }
}
```

## 7. On Failure

When a step fails, the engine checks for a per-step `failure_step`. If set,
the error is recorded but execution continues.

If no per-step `failure_step` exists, the engine checks the
`FlowDefinition.on_failure` field. If set, it creates a synthetic step of
kind `FailureModule` with the on_failure flow path as its target.

```json
{
  "path": "data.pipeline", "version": "0.1.0",
  "summary": "ETL pipeline with failure handling",
  "on_failure": "notifications.on_pipeline_failure",
  "steps": [
    { "id": "extract", "kind": "Script", "script_path": "data.extractor" },
    { "id": "load", "kind": "Script", "script_path": "data.loader",
      "depends_on": ["extract"] }
  ]
}
```

If `extract` fails and has no `failure_step`, the engine:

1. Records the error on the `extract` step
2. Marks `extract` as completed (with error context)
3. Inserts a synthetic `__on_failure__` step running the failure flow
4. Execution continues (the failure flow inspects the error)

The failure flow receives this context:

```json
{
  "status": "failure_handler_ready",
  "handled_error": "extract failed: connection refused",
  "for_step": "extract",
  "handler": "notifications.on_pipeline_failure"
}
```

If neither per-step `failure_step` nor flow-level `on_failure` is set, a
step failure propagates as an error from the entire execution.

## 8. Parallel Execution

The engine dispatches independent steps concurrently via a round-based loop:

1. Start with all steps as `pending`
2. Each round, partition into `ready` and `not_ready`:
   - A step is ready when all its `depends_on` IDs are in `completed`
   - Steps with unmet dependencies stay pending
3. If no steps are ready, detect a deadlock and break
4. Dispatch all ready steps via `futures::future::join_all`
5. Collect results, add step IDs to `completed`
6. Repeat until all steps are done or the round limit (1000) is reached

Complex steps (Branch, Loop, CallFlow, FailureModule) use a snapshot-based
approach: each captures a clone of the current results state at dispatch
time, avoiding shared state across concurrent branches.

Round-based dispatch example for a flow with four steps:

```
Pending: [A, B, C, D]
Round 1: ready=[A], not_ready=[B, C, D]
  A runs (B, C, D depend on A)
Round 2: ready=[B, C], not_ready=[D]
  B and C run in parallel (D depends on both)
Round 3: ready=[D], not_ready=[]
  D runs
```

## 9. Telemetry

The engines `execute_with_telemetry` method returns both step results and a
per-step telemetry log. Each entry records:

```json
{
  "step_id": "fetch_data",
  "step_kind": "script",
  "status": "Completed",
  "started_at": "2026-05-07T14:30:00Z",
  "completed_at": "2026-05-07T14:30:02Z",
  "duration_ms": 2340,
  "retry_attempt": 0,
  "output": { "rows": 150 }
}
```

Status values:

- `Pending` -- not yet started
- `Running` -- currently executing
- `Completed` -- finished successfully
- `Failed(reason)` -- finished with an error
- `Skipped(reason)` -- skipped via `stop_if`
- `TimedOut` -- exceeded `timeout_ms`

An optional `progress_callback` is invoked after each step completes with
the completed count, total count, and the steps telemetry record.

```json
[
  { "step_id": "extract", "status": "Completed", "duration_ms": 1200 },
  { "step_id": "validate", "status": "Completed", "duration_ms": 800 },
  { "step_id": "transform", "status": "Skipped", "reason": "no_data" }
]
```

## 10. Complete Example

A realistic pipeline combining Shell, BranchOne, ForLoop, CallFlow, Sleep,
dependency ordering, and on_failure.

```json
{
  "path": "social.daily_pipeline",
  "version": "0.5.0",
  "summary": "Daily social media aggregation pipeline",
  "default_retry": {
    "max_attempts": 3, "delay_ms": 1000, "backoff": "exponential"
  },
  "default_timeout_ms": 30000,
  "on_failure": "notifications.alert_operator",
  "tags": ["social", "daily", "aggregation"],
  "steps": [
    {
      "id": "fetch_posts",
      "kind": "Shell",
      "command": "curl -s https://api.social.example/posts?since=yesterday",
      "timeout_ms": 15000
    },
    {
      "id": "fetch_analytics",
      "kind": "Shell",
      "command": "curl -s https://api.social.example/analytics/daily",
      "timeout_ms": 15000
    },
    {
      "id": "classify_content",
      "kind": "BranchOne",
      "depends_on": ["fetch_posts"],
      "branches": [
        [
          {
            "id": "classify_ml",
            "kind": "Script",
            "script_path": "ml.content_classifier",
            "input": { "source": "${fetch_posts.stdout}" }
          }
        ],
        [
          {
            "id": "classify_fallback",
            "kind": "Shell",
            "command": "echo '${fetch_posts.stdout}' | classify.sh"
          }
        ]
      ]
    },
    {
      "id": "process_each",
      "kind": "ForLoop",
      "depends_on": ["classify_content"],
      "iterator": "posts",
      "input": { "posts": "${classify_content.results}" },
      "steps": [
        {
          "id": "enrich_post",
          "kind": "CallFlow",
          "flow_path": "social.enrich_post",
          "input": {
            "post_id": "${posts__item.id}",
            "category": "${posts__item.category}"
          }
        }
      ]
    },
    {
      "id": "generate_report",
      "kind": "Shell",
      "depends_on": ["fetch_analytics", "process_each"],
      "command": "build_report.sh --analytics '${fetch_analytics.stdout}' --posts '${process_each.iterations}'",
      "stop_if": "${fetch_analytics.exit_code} != 0",
      "failure_step": "notify_partial"
    },
    {
      "id": "notify_partial",
      "kind": "Shell",
      "command": "notify.sh 'Report generation skipped due to analytics failure'"
    },
    {
      "id": "sleep_before_archive",
      "kind": "Sleep",
      "depends_on": ["generate_report"],
      "sleep_after_ms": 5000
    },
    {
      "id": "archive",
      "kind": "CallFlow",
      "depends_on": ["sleep_before_archive"],
      "flow_path": "storage.archive_results",
      "input": { "source": "daily_report" }
    }
  ]
}
```

What this pipeline does:

1. Two root steps (`fetch_posts`, `fetch_analytics`) run in parallel
2. `classify_content` uses BranchOne: try the ML module first, fall back
   to a shell script if no compiled binary exists
3. `process_each` iterates over classified results, calling a sub-flow for
   each post via CallFlow
4. `generate_report` waits for both `fetch_analytics` and `process_each`.
   It checks `stop_if`: if analytics returned non-zero exit code, the step
   is skipped. On other failures, `failure_step` routes to `notify_partial`
5. `notify_partial` runs only if `generate_report` fails with its failure
   step set
6. `sleep_before_archive` pauses for 5 seconds
7. `archive` calls the `storage.archive_results` sub-flow via CallFlow

If any step fails without its own `failure_step`, the engines
`on_failure: "notifications.alert_operator"` creates a synthetic
FailureModule step that executes the alert flow. The pipeline continues
rather than aborting.
