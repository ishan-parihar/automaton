# Automaton Architecture

## Section 1: System Overview

Automaton is an AI-native automation substrate written in Rust. It is designed
around a single premise: AI agents should be able to create, compose, and execute
modular automation workflows without leaving their chat interface. The entire
system surface is exposed through an MCP (Model Context Protocol) server built
on the official `rmcp` crate, giving LLM agents direct access to 39 tools
spanning module lifecycle management, property graph querying, workflow planning,
DAG execution, secret management, resource binding, cron scheduling, webhook
configuration, and run telemetry.

The architecture is structured as 15 interdependent crates organized around a
three-layer design. At the foundation lies `automaton-core`, which defines the
shared type system: `Flow`, `FlowStep`, `FlowStepKind` (9 variants including
Script, Shell with command/shell fields, BranchOne, BranchAll, ForLoop,
WhileLoop, Sleep, FailureModule, and CallFlow with recursive flow_path/input),
the `NodeKind` enum (12 variants: Module, Workflow, Trigger, Resource, SecretRef,
Capability, Artifact, Run, Observation, Constraint, AlternativePath, Input),
the `EdgeKind` enum (10 variants: DependsOn, Calls, Emits, Consumes, Triggers,
UsesResource, BlockedBy, AlternativeTo, Upgrades, DerivedFrom), `ExecutionId`,
`StepTelemetry`, `StepStatus`, `FlowExecution`, `WebhookEvent`, and the
`RegistryBackend` trait (20 async methods).

The orchestration layer (`automaton-engine`) provides both module-based DAG
execution via `Engine` (plan, materialize, execute) and flow-definition-based
execution via `FlowEngine` (flatten, execute, execute_with_telemetry). The
storage layer uses a dual-backend pattern abstracted behind `RegistryBackend`,
with `automaton-registry` providing SQLite via rusqlite and `automaton-postgres`
providing Postgres via sqlx. A separate `automaton-graph` crate manages the
property graph store with nodes and edges tables, json_extract queries,
pathfinding via DFS, time-range scanning, text search via LIKE, and GROUP BY
summarization.

Modules are authored in Rust using the `#[automaton]` proc-macro from
`automaton-sdk`, compiled to native binaries by `automaton-build` with
content-addressed caching (SHA-256), and executed as child processes by
`automaton-runtime` with configurable timeout, retry, and backoff. Background
processing is handled by `automaton-scheduler` (cron expression evaluation via
the croner crate with CronTicker minute-level deduplication and SchedulerDaemon
background polling) and `automaton-worker` (concurrent job queue processing with
per-task Registry handles for SQLite WAL compatibility). The `automaton-cli`
binary ties everything together with 14+ subcommands, and `automaton-api`
provides an Axum-based HTTP server with JWT auth and rate limiting for production
deployments where REST access is needed alongside MCP.

## Section 2: Crate Map

| Crate | Role | Description |
|---|---|---|
| automaton-core | Shared types | Foundation crate with zero runtime dependencies on other automaton crates. Defines `Flow`, `FlowStep`, `FlowStepKind` (9 variants), `FlowDefinition` (path, version, summary, steps, default_retry, default_timeout_ms, on_failure, tags), `Node` (id, kind, name, properties, created_at), `Edge` (id, source, target, kind, properties, created_at), `NodeKind` enum (12 variants), `EdgeKind` enum (10 variants), `ModuleNode` (id, module_id, input, retry, timeout_ms, depends_on, parallel_group, condition, error_handler), `RunGraph` (id, workflow_name, modules, steps, created_at), `ExecutionId` (UUID newtype with Display), `StepStatus` enum (Pending, Running, Completed, Failed, Skipped, TimedOut), `StepTelemetry` (step_id, step_kind, status, started_at, completed_at, duration_ms, retry_attempt, output, error), `FlowExecution` (execution_id, flow_path, dag_label, steps, started_at, completed_at, status, total_duration_ms), `WebhookEvent` enum (8 variants), `WebhookRegistration` (id, target_url, event, secret, enabled, created_at), `RegistryBackend` trait (20 async methods), `AutomatonError`, `AutomationManifest` (name, version, entry, summary, description, timeout_ms, retry, permissions, depends_on, resources, tags, require_approval, inputs_schema, outputs_schema), `ModuleId` (path, version, hash, created_at), `ContentHash` (SHA-256 hex string newtype), `RetryConfig` (max_attempts, delay_ms, backoff), `BackoffKind` enum (Fixed, Linear, Exponential), `DepRef` (name, version_req), `SchemaMode` enum (Auto, Strict, None). |
| automaton-engine | Orchestrator | Contains two engines. `Engine` holds `Arc<dyn RegistryBackend>`, `GraphStore`, `Runtime`. Methods: `plan(&self, start_module, &PlanOptions) -> RunGraph` (DFS dependency discovery with visited set and max_depth), `materialize(&self, &RunGraph) -> ExecutableDag` (builds `petgraph::DiGraph<ExecNode, ()>`, adds edges for depends_on, verifies acyclicity via `petgraph::algo::toposort`), `execute(&self, ExecutableDag) -> RunResult` (computes topological levels, groups by level, runs concurrently via `futures::join_all`, resolves `$var:`/`$res:` references and `${module_path.field}` cross-step refs, records runs). `PlanOptions` (max_depth, include_alternatives, dry_run). `ExecNode` (id, module_name, input, retry, timeout_ms, state). `ExecutableDag` (graph: DiGraph, node_indices, run_graph_id). `RunResult` (run_graph_id, results, flow_state, completed_at). `FlowEngine` static methods: `flatten(&FlowDefinition) -> Vec<ExecStep>` (recursively walks steps, handles branches/loops/CallFlow with body_ids, enforces depth 10), `execute(steps, backend, runtime, build_cache_dir)` (work-stealing loop with ready/blocked separation, concurrent dispatch, stop_if evaluation, failure routing), `execute_with_telemetry(...)` (same but collects StepTelemetry per step with optional progress_callback). Shell steps use `tokio::process::Command` with `kill_on_drop(true)` to prevent orphan processes. |
| automaton-graph | Graph store | SQLite-backed property graph via rusqlite. `GraphStore` holds `Mutex<Connection>`. Opened via `open()` (separate graph.db) or `open_merged()` (shares registry.db). Schema: `nodes` table (id TEXT PK, kind TEXT, name TEXT, properties TEXT, created_at TEXT), `edges` table (id TEXT PK, source TEXT FK, target TEXT FK, kind TEXT, properties TEXT, created_at TEXT). Indexes on nodes.kind, nodes.name, nodes.created_at, edges.source, edges.target, edges.kind, edges.created_at. Methods: add_node, add_edge, get_node, all_nodes, all_edges, all_nodes_paginated, find_nodes_by_kind, find_nodes_by_kind_paginated, find_nodes_by_properties (uses `json_extract()` for server-side JSON filtering), search_nodes (LIKE substring match), find_nodes_in_time_range (ISO 8601 inclusive), find_edges_in_time_range (JOINs edges with nodes), find_path (DFS, max depth 10, returns all paths as Vec<Vec<NodeAndEdge>>), get_dependency_chain (BFS), get_outgoing_edges, get_incoming_edges, summarize (GROUP BY totals and breakdowns), delete_node (cascades to edges), delete_edge. |
| automaton-mcp | MCP server | Implements `rmcp::ServerHandler` with 39 tools. `add_tool()` helper registers tools with name, description, and JSON schema from `schemars::schema_for!()`. All param structs derive `Deserialize` and `JsonSchema` with `#[serde(deny_unknown_fields)]`. Dispatch: `parse_args()` deserializes arguments into typed struct, handler calls engine methods, returns `ok_json(value)` or `err_json(msg)`. Tools: module_create/build/validate/run/deprecate/search/template/list_templates, workflow_plan/materialize, graph_query/pathfind/add_edge/search/time_range/summarize, flow_create/show/execute/execute_telemetry/list/delete, schedule_create/validate, webhook_register/list/delete, secret_set/get, resource_bind/list, job_queue/list, run_logs/retry, registry_search, capability_inventory, system_health. `flow_execute_telemetry` sends MCP progress notifications via `notify_progress` using a callback from `execute_with_telemetry`. |
| automaton-api | REST API | Axum-based HTTP server with 18+ routes. Routes: `GET/POST /api/scripts`, `GET /api/scripts/:path`, `POST /api/scripts/:path/build`, `POST /api/scripts/:path/run`, `GET/POST /api/jobs`, `GET /api/runs`, `GET/POST /api/variables`, `GET /api/variables/:path`, `GET/POST /api/resources`, `GET /api/resources/:path`, `POST /api/webhooks/:trigger_id` (validates webhook secret), `POST /api/events/:trigger_id` (validates event_source), `GET/POST /api/triggers`, `POST /api/graph/nodes`, `POST /api/graph/edges`. JWT auth middleware validates HS256 Bearer tokens when `AUTOMATON_JWT_SECRET` is set. Rate limiting via `Arc<Semaphore>` with configurable MAX_CONCURRENT_REQUESTS (default 100), returns 429 when exhausted. Backed by `automaton_postgres::AutomatonDb`. |
| automaton-cli | CLI binary | 14+ subcommands via clap. `Init` creates workspace directories. `New` scaffolds module from template with source + automation.yaml. `Build` compiles via BuildCache. `Run` executes compiled binary via Runtime. `Graph` with subcommands Nodes/Edges/Path/Deps. `Plan` calls Engine::plan. `Execute` chains plan+materialize+execute. `Mcp` starts MCP stdio server. `List` queries registry. `Show` module details with manifest. `Logs` run history. `Retry` schedules retry. `Worker` starts scheduler+worker (with `--daemon` flag). `Doctor` system diagnostics. `Postgres Migrate` runs schema migrations. `init_engine()`: opens Registry, opens merged GraphStore, creates Runtime, returns Engine. `RegistryTriggerProvider` adapter wraps Registry for SchedulerDaemon. |
| automaton-registry | SQLite backend | Implements all 20 RegistryBackend methods for SQLite. `Registry` holds `Mutex<Connection>`, build_cache path, optional `SecretKeeper`. Schema (12 tables): modules (path PK, hash UNIQUE), dependencies (composite PK), builds, runs, resources, variables, triggers (cron/webhook/event), graph_nodes, graph_edges, flows, jobs (AUTOINCREMENT PK, running guard), webhooks, executions. `resolve_references()` replaces `$var:path` and `$res:path` patterns. `SecretKeeper` uses 64-hex-char `AUTOMATON_MASTER_KEY` for AES encryption. `register()` computes SHA-256 hash from source. `dequeue()` uses LIMIT 1 with running flag update-and-check-changes. |
| automaton-postgres | Postgres backend | Implements RegistryBackend via sqlx. `AutomatonDb` holds `PgPool` (max 20 connections). Migration on connect: scripts (hash PK, path, version, source, manifest JSONB, built bool), script_deps, flows, jobs (FOR UPDATE SKIP LOCKED for safe concurrent dequeuing), runs, graph_nodes, graph_edges, variables, resources, triggers, builds, webhooks, executions. Cross-implements RegistryBackend by adapting its script/job/run methods. |
| automaton-runtime | Process sandbox | `Runtime` with `RuntimeConfig` (work_dir, temp_dir, default_timeout_ms, max_concurrency). `run_binary()` spawns binary with `--input <json>`, pipes stdout/stderr, enforces timeout, parses JSON output. `run_with_retry()` wraps run_binary with retry loop. Backoff: Fixed (constant), Linear (delay * (attempt+1)), Exponential (delay * (1 << attempt)). |
| automaton-build | Build cache | `BuildCache::build_rust()` computes SHA-256 hash, creates temp Cargo project (Cargo.toml with serde/serde_json/tokio/anyhow + optional automaton-sdk path dep), runs `cargo build --release`, caches binary by hash. `diagnose()` parses cargo stderr into BuildDiagnostic structs. 10 template patterns: echo, http-fetch, http-server, db-query, slack-notify, data-transform, health-check, rate-limiter, file-watch, cron-worker. |
| automaton-scheduler | Cron scheduler | `Scheduler::validate()` and `Scheduler::matches()` via croner. `CronTicker` deduplicates fires (tracks last_minute). `TriggerProvider` trait (get_cron_triggers, enqueue_job). `SchedulerDaemon::start()` spawns background tokio task that polls provider, maintains `Vec<(id, CronTicker)>`, fires matching triggers by enqueuing jobs. AtomicBool shutdown. |
| automaton-worker | Job processor | `Worker` with name, concurrency, BuildCache, Runtime, AtomicBool shutdown. `run_module()` builds via BuildCache and executes via Runtime with retry. `process_job()` dequeues, looks up module, builds if needed, executes, records run, completes job. `start()` runs single-worker loop or N concurrent tasks (each with own Registry handle for SQLite WAL). |
| automaton-daemon | Unified daemon | Not a separate crate. The CLI's `Worker` subcommand starts both SchedulerDaemon and Worker in one process. `--daemon` flag forks a background child process and writes PID to worker.pid. |
| automaton-sdk | Proc-macro SDK | Re-exports `automaton_sdk_derive::automation` and core types. `prelude` module: `automation` macro, `AutomationManifest`, `BackoffKind`, `ContentHash`, `DepRef`, `ModuleId`, `RetryConfig`, `JsonSchema`, `Serialize`, `Deserialize`. `Context` struct with run_id (UUID string), module_name (from CARGO_PKG_NAME), attempt (1-based). |
| automaton-sdk-derive | Proc macro | `#[automaton]` attribute macro. Parses `fn main(ctx: Context, input: T) -> Result<U>`. Generates entrypoint: reads `--input <json>` from CLI args or stdin, deserializes to T, calls user function, serializes output, prints to stdout. Generates `__automaton_input_schema()` and `__automaton_output_schema()` via `schemars::schema_for!`. Supports both async and sync functions. |
| automaton-tests | Integration tests | Custom test harness (main() with manual pass/fail tracking). 28+ tests covering: flow flattening (simple sequences, branches, for-loops), shell execution (basic commands, output capture, exit code, stop_if, failure_step, dependency order, retry, timeout), pipeline DAG execution with cross-step state, graph CRUD operations, pathfinding, summarize. Helpers: AutoRuntime for temp cleanup, shell_step constructors, test_ok/err/eq assertions. |

## Section 3: Data Flow

The lifecycle of an automation module moves through six stages from creation to
execution output.

Stage one is registration. An agent calls `module_create` (MCP) or runs
`automaton new` (CLI). The system computes a SHA-256 content hash from the
source bytes, deserializes the `AutomationManifest`, and calls
`Registry::register()` which inserts a row into the `modules` table and
dependency rows into the `dependencies` table. A `Node` with
`NodeKind::Module` is added to the property graph. The returned `ModuleId`
contains the path, parsed semantic version, content hash, and timestamp.

Stage two is building. The source is compiled by `BuildCache::build_rust()`
into a native binary. A temporary Cargo project is created with a generated
`Cargo.toml` (serde, serde_json, tokio, anyhow, and optionally automaton-sdk as
a path dependency). `cargo build --release` is invoked. On success, the binary
is copied to a content-addressed cache path (`builds/<hash>/binary`) and to a
predictable path (`builds/<sanitized_name>`). The registry's `modules.built`
flag is set to 1, and a build record is inserted into the `builds` table.

Stage three is registry storage. The built module remains in the registry
indexed by path. The content hash serves as the cache key: unchanged source
skips rebuilding. The module's manifest, including its name, version, summary,
entry point, timeout, retry config, permissions, resource bindings, dependency
list, tags, and approval requirements, is stored as serialized JSON in the
`modules.manifest` column. The `modules.built` boolean flag distinguishes
registered-only modules from compiled ones. Metadata is available via
`module_search`, `registry_search`, `show`, and `list` at any point after
registration.

Stage four is planning. For workflow execution, `Engine::plan()` performs a DFS
from the starting module path. It fetches the root module from the registry to
confirm it exists, then recursively walks the dependency tree via an explicit
stack of (path, depth) tuples. A `HashSet<String>` visited set prevents infinite
loops from circular dependency references. Each discovered module becomes a
`ModuleNode` in a `RunGraph`, annotated with a unique execution node ID (path
plus a truncated UUID), the module's parsed semver version, its dependency list
as declared in the manifest, the retry configuration, timeout, and an empty
input placeholder. `PlanOptions.max_depth` (default 10) limits how deep the DFS
will traverse, protecting against excessively deep dependency trees.

Stage five is materialization. `Engine::materialize()` converts the `RunGraph`
into an `ExecutableDag` backed by `petgraph::DiGraph<ExecNode, ()>`. Dependency
edges are added, and `petgraph::algo::toposort` verifies acyclicity. A cycle
returns `AutomatonError::CyclicDependency`.

Stage six is execution. `Engine::execute()` computes topological levels by
running `petgraph::algo::toposort` to obtain a topological ordering, then
measuring each node's longest path from root nodes. Nodes whose level has no
incoming dependencies are level 0; each subsequent level increments by 1. Nodes
are grouped by level in a `BTreeMap<usize, Vec<NodeIndex>>` for guaranteed
level-order iteration. For each level, execution proceeds in three phases. Phase
one resolves all inputs by calling `backend.resolve_references()` for `$var:`
and `$res:` substitution, which recursively walks the input JSON and replaces
`$var:path` with stored variable values and `$res:path` with stored resource
JSON. Phase two further resolves cross-step references via `resolve_state_refs()`
for `${module_path}` and `${module_path.field}` patterns against the accumulated
`flow_state` HashMap. Phase three dispatches all nodes in the level concurrently
via `futures::join_all`, each calling `Runtime::run_binary()` or
`Runtime::run_with_retry()`. After each level completes, outputs are recorded
via `backend.record_run()` and `backend.update_run()`, stored in `flow_state`
keyed by module path, and node states are updated to `Completed` or `Failed`.
Failed modules do not halt execution; their error is stored in `flow_state` for
downstream reference. The final `RunResult` contains the run graph ID,
per-module results, accumulated flow state, and completion timestamp.

For flow-based execution, `FlowEngine::execute()` uses a work-stealing loop with
a `VecDeque<usize>` of pending step indices. Each iteration separates ready
steps (all `depends_on` satisfied) from blocked steps. Ready steps undergo
`stop_if` evaluation: if the condition resolves to true, the step is skipped
with a `Skipped` status and `{"status":"skipped","reason":"stop_if_triggered"}`
output. Remaining ready steps are dispatched concurrently. BranchOne tries
branches in order, running each branch's steps until one succeeds. BranchAll
runs all branches and collects results as a JSON array. ForLoop iterates over
resolved iterator values, tracking `iter__item` and `iter__index` in
local_results per iteration. WhileLoop evaluates condition strings against
accumulated state with support for `${x.status} == "completed"` and
`${x.count} > 0` patterns, running up to `max_iterations`. CallFlow recursively
fetches the referenced flow from the registry, deserializes the `FlowDefinition`,
calls `flatten()` and `execute_with_handlers()`, and merges all child step
results into the parent flow's state map.

## Section 4: MCP Integration

The MCP server in `automaton-mcp` is the primary interface for AI agents. It
implements `rmcp::ServerHandler` with a single `call_tool` dispatch method that
routes `CallToolRequestParams` by tool name. The server sets
`ServerCapabilities::builder().enable_tools()`.

Tool registration happens in `list_tools()` via `add_tool()`, which constructs
`Tool::new(name, description, schema)`. Every param struct derives both
`Deserialize` and `JsonSchema` with `#[serde(deny_unknown_fields)]` to reject
hallucinated parameters. The `schema_for::<T>()` function generates a JSON
Schema via `schemars::schema_for!` and wraps it in `Arc<JsonObject>`. This
ensures accurate typed schemas in the `list_tools` response.

Dispatch follows a consistent pattern across all 39 tools. Each arm in the
`call_tool` match statement calls `parse_args::<T>(&request)` which extracts
`request.arguments` as a `serde_json::Map`, wraps it in `Value::Object`, and
deserializes to the target type `T` using serde. If deserialization fails (e.g.,
a required field is missing or a type is wrong), the function returns an
`ErrorData` with code -32602 (Invalid Params). On successful deserialization,
the handler calls the appropriate backend, engine, or graph store method, then
returns the result via `ok_json(value)` which wraps the JSON value in
`CallToolResult::success` with pretty-printed JSON text content, or via
`err_json(msg)` which wraps an error message in `CallToolResult::error`.
This uniform dispatch pattern means every tool has the same error handling
semantics and response format, making it predictable for AI agents to consume.

The `flow_execute_telemetry` tool demonstrates advanced MCP capabilities beyond
simple request-response. It extracts the optional `progress_token` from the
incoming request via `request.progress_token()`, clones the `Peer` handle from
the request context into an `Arc<Mutex<Peer>>`, and constructs a progress
callback closure. This closure is passed to `FlowEngine::execute_with_telemetry()`
as the `progress_callback` parameter. As each step completes during flow
execution, the callback calls
`peer.notify_progress(ProgressNotificationParam::new(token, current).with_total(total))`,
which sends an MCP notification to the client with the current and total step
counts. The notification is dispatched via `tokio::spawn` to avoid blocking the
execution loop. This pattern enables AI agents to display real-time progress
bars and step-by-step updates during long-running workflow executions.

The `graph_query` tool demonstrates flexible property filtering. When the
`properties` parameter is provided, the tool first fetches nodes by kind (or all
nodes if no kind filter), then applies in-memory property matching against the
JSON `properties` field of each `Node` struct. When no properties are specified,
it uses paginated SQL queries (`LIMIT`/`OFFSET`) for efficient server-side
paging. The `graph_time_range` tool runs two separate SQL queries: one for nodes
created within the range and one for edges created within the range (via a JOIN
between edges and nodes to return `NodeAndEdge` results). Both queries use
parameterized ISO 8601 datetime bounds.

The `system_health` and `capability_inventory` tools provide self-discovery,
returning version, module count, graph node/edge counts, resource types, and
tool count (39). These enable AI agents to dynamically understand the system's
capabilities.

## Section 5: Storage Layer

Automaton uses a dual-backend storage architecture abstracted behind the
`RegistryBackend` trait in `automaton-core`. The trait defines 20 async methods
covering module CRUD, run recording, variable/resource resolution, job queue
operations, trigger management, flow CRUD, webhook management, and execution
history. Two implementations exist: `automaton-registry::Registry` (SQLite via
rusqlite) and `automaton-postgres::AutomatonDb` (Postgres via sqlx).

The SQLite backend stores all data in `registry.db`. Its schema includes 12
tables. The `modules` table uses path as primary key with a unique constraint on
the SHA-256 hash. The `dependencies` table has a composite primary key of
(module_path, depends_on). The `jobs` table uses `AUTOINCREMENT` with a
`running` boolean and worker_id for race protection. The `triggers` table has
`enabled` boolean and `trigger_type` (cron, webhook, event) with a JSON
`config` column. The `variables` table supports optional AES encryption via
`SecretKeeper` (activated by `AUTOMATON_MASTER_KEY` env var, 64 hex chars). The
`resources` table stores typed JSON values. Indexes exist on `nodes.kind`,
`nodes.name`, `nodes.created_at`, `edges.source`, `edges.target`, `edges.kind`,
and `jobs.scheduled_for` (WHERE NOT running).

The Postgres backend mirrors this schema with JSONB columns and TIMESTAMPTZ
types. Job dequeuing uses `FOR UPDATE SKIP LOCKED` in a subquery for atomic
claiming, more robust than SQLite's update-and-check-changes approach. The
connection pool is configured for max 20 connections. Schema migration runs
automatically on connect.

The `automaton-graph` crate manages the property graph separately. Its
`GraphStore` uses `find_nodes_by_properties()` with SQLite's `json_extract()`
function for server-side JSON filtering, avoiding loading all nodes into memory.
`find_path()` implements DFS-based pathfinding with a visited set and max depth
of 10, returning all discovered paths. `summarize()` uses GROUP BY queries for
aggregate statistics. The graph store can be opened in merged mode
(`open_merged()`) sharing `registry.db`, eliminating the separate `graph.db`
file.

## Section 6: Engine Internals

The `Engine` struct holds `Arc<dyn RegistryBackend>`, `GraphStore`, and
`Runtime`. The `plan()` method performs DFS from a root module using a visited
set, constructing `ModuleNode` structs with unique IDs, parsed semver versions,
retry configs, and dependency lists. The resulting `RunGraph` is a flat vector
of discovered modules.

The `materialize()` method creates a `petgraph::DiGraph<ExecNode, ()>` and
populates it by iterating over the `RunGraph.modules` vector. Each `ModuleNode`
becomes an `ExecNode` with an initial `ExecutionState::Pending`. Node indices
are tracked in a `HashMap<String, NodeIndex>` keyed by the module's unique
execution node ID. After all nodes are added, the method iterates each module's
`depends_on` list, finds the corresponding node by matching `module_id.path` to
the dependency name, and adds a directed edge from the dependency to the
dependent via `dag.add_edge(source_idx, target_idx, ())`. After all edges are
placed, `petgraph::algo::toposort` is called on the graph. If toposort returns
`Err`, the graph contains a cycle and `AutomatonError::CyclicDependency` is
returned to the caller. The resulting `ExecutableDag` preserves the graph, the
node index map, and the run graph ID for downstream execution.

The `execute()` method begins by re-running `toposort` to obtain a topological
ordering of `NodeIndex` values. It then computes levels for each node by
measuring the longest path from root nodes: each node's level is the maximum
level of its incoming neighbors plus one, or zero for root nodes with no
incoming edges. Levels are stored in a `HashMap<NodeIndex, usize>` and nodes
are grouped by level in a `BTreeMap<usize, Vec<NodeIndex>>` to guarantee
level-order iteration from root to leaves. Each level group is processed
sequentially, but all nodes within a level run concurrently. Before execution,
inputs undergo two resolution passes. First, `backend.resolve_references()`
walks the input JSON recursively, replacing `$var:path` with stored variable
values and `$res:path` with stored resource JSON, with unresolved references
returning the original string. Second, `resolve_state_refs()` replaces
`${module_path}` with the entire output of a completed module and
`${module_path.field}` with a specific field, using the accumulated `flow_state`
HashMap from previous levels. Each node then dispatches via
`Runtime::run_binary()` or `Runtime::run_with_retry()` depending on the
module's `RetryConfig`. On completion, outputs are recorded via
`backend.record_run()` and `backend.update_run()`, stored in `flow_state` keyed
by module path, and node states are updated. Failed nodes do not halt execution;
their errors are stored in `flow_state` for downstream reference. The method
returns `RunResult` containing the run graph ID, per-module results, accumulated
flow state, and completion timestamp.

The `FlowEngine::flatten()` method recursively walks `FlowDefinition.steps`,
converting each `FlowStep` to an `ExecStep`. BranchOne and BranchAll inject
control marker steps with nested branch step IDs. ForLoop and WhileLoop inject
control steps with `body_ids` and `body_steps`. CallFlow creates a step that
dynamically fetches the referenced flow at runtime. Recursion depth is limited
to 10.

The `FlowEngine::execute()` method uses a work-stealing loop with a `VecDeque`.
Each iteration checks `depends_on` satisfaction. `stop_if` conditions are
evaluated before dispatch; satisfied conditions skip the step with `Skipped`
status. Shell steps spawn `tokio::process::Command` with `kill_on_drop(true)`
for orphan prevention. BranchOne tries branches in order until one succeeds.
BranchAll runs all branches and collects results as a JSON array. ForLoop
iterates with per-item state (`iter__item`, `iter__index`). WhileLoop supports
`==` and `>` operators with state reference resolution. CallFlow fetches the
flow definition from the registry, calls `flatten()` and
`execute_with_handlers()` recursively, and merges results.

`execute_with_telemetry()` extends `execute()` with per-step `StepTelemetry`
collection (timing, status, output, error) and an optional progress callback
that receives `(completed_count, total_count, &StepTelemetry)`. The MCP server
uses this callback to send `notify_progress` notifications.

## Section 7: CLI & API

The CLI binary uses clap with `#[derive(Parser)]`. The `Commands` enum defines
14+ variants. `Init` creates the workspace structure. `New` validates the module
path, writes source from template, generates `automation.yaml` manifest via
serde_yaml, registers the module, and adds a graph node. `Build` fetches the
module and compiles via `BuildCache::build_rust()`. `Run` looks up the compiled
binary and executes via `Runtime::run_binary()`. `Graph` subcommands call
`all_nodes()`, `all_edges()`, `find_path()`, and `get_dependency_chain()`.
`Plan` calls `Engine::plan()`. `Execute` chains plan, materialize, and execute.
`Mcp` starts the MCP stdio server. `List` queries `list_modules()`. `Show`
fetches full module details. `Logs` retrieves run history. `Worker` starts both
SchedulerDaemon and Worker, optionally daemonizing. `Doctor` runs system
diagnostics. `Postgres Migrate` connects and migrates.

The REST API is built with Axum. `create_router()` constructs 18+ routes under
`/api/`. Auth middleware reads `AUTOMATON_JWT_SECRET` and validates HS256 Bearer
tokens. Rate limiting middleware uses `Arc<Semaphore>` with configurable
`MAX_CONCURRENT_REQUESTS` (default 100), returning 429 when exhausted. Handler
functions follow a consistent extract-parse-return pattern. Webhook and event
handlers validate trigger configuration (secret matching, event source matching)
before enqueuing jobs. The API is designed for Postgres-backed production
deployments.

## Section 8: Deployment

Automaton compiles to a single static musl binary via `cargo build --release
--target x86_64-unknown-linux-musl`. This binary includes all 15 crates, the
SQLite backend via rusqlite with bundled SQLite, the MCP server, the build
cache, the scheduler, and the worker. No runtime dependencies beyond the
operating system are required.

In local development mode, the SQLite backend stores all data in
`~/.local/share/automaton/`. The `registry.db` file contains both registry and
graph tables in a merged schema. Build artifacts are cached under `builds/`
with content-addressed paths. No external database or message queue is needed.

For production, the Postgres backend replaces SQLite. The CLI connects via
`automaton postgres migrate --database-url` or the `DATABASE_URL` environment
variable. The REST API server starts via `automaton_api::serve()`, connecting to
Postgres, creating the Axum router with auth and rate limiting, and binding a
TCP listener. This enables HTTP-based integration alongside MCP-based AI agent
access.

The daemon mode (`automaton worker --daemon`) runs both scheduler and worker in
a single process. The scheduler polls for enabled cron triggers and enqueues
jobs. The worker dequeues and executes them. The `--daemon` flag forks a
background process that writes its PID to `worker.pid` and detaches. Multi-worker
deployments use `--name` and `--concurrency` flags. Each concurrent task opens
its own SQLite connection handle, relying on WAL mode for safe concurrent
access. Graceful shutdown is supported via `AtomicBool` checked on each
iteration of the worker pull loop.
