# MCP Tool Reference

Automaton exposes 38 tools via the Model Context Protocol (MCP). This reference documents every tool's parameters, behavior, and example usage. Tools are organized into 9 categories covering module lifecycle, workflow planning, graph manipulation, flow composition, scheduling, secrets management, resource binding, webhooks, and system introspection.

All param structs enforce `#[serde(deny_unknown_fields)]` to prevent incorrect field names. Tools use a parse_args() pattern for input and return either ok_json() or err_json() for output. Pagination via limit/offset is available on list and search tools.

## Tool Overview

| Category | Tools |
|----------|-------|
| Modules | create, build, validate, run, deprecate, search, template, template_list |
| Workflows | plan, materialize |
| Graph | query, pathfind, add_edge, summarize, search, time_range |
| Flows | create, show, execute, execute_telemetry, list, delete |
| Schedules | create, validate |
| Secrets | set, get |
| Resources | bind, list |
| Jobs | queue, list |
| Runs | logs, retry |
| Registry | search |
| Webhooks | register, list, delete |
| System | capability_inventory, system_health |

## Architecture Note

The MCP server is built on the `rmcp` crate and communicates over stdio transport. It wraps the Automaton engine, which provides planning, materialization, and execution. The backend registry (SQLite or Postgres) stores modules, builds, runs, webhooks, secrets, and resources. The property graph (SQLite) tracks entities and their relationships. The runtime manages child process execution with sandboxing, timeouts, and retry logic.

## Modules (8 tools)

Module lifecycle tools manage automation units from creation through deprecation. Each module is a Rust source file with an associated manifest (AutomationManifest) that declares the module's version, summary, dependencies (depends_on), execution timeout (timeout_ms), and resource requirements.

Modules are content-addressed in the registry using BLAKE3 hashing, enabling deterministic identity and caching. They can be compiled to binaries (module_build), executed standalone (module_run), searched (module_search), validated (module_validate), removed (module_deprecate), or scaffolded from templates (module_template, template_list). Modules can also be composed into multi-step flows via flow_create.

The typical module lifecycle is: create -> build -> run (iterated during development), then compose into flows for production use. Modules can declare dependencies on other modules via the depends_on field, which the workflow planner uses to build execution graphs.

### module_create
Register a new automation module with Rust source code and a manifest that declares metadata, dependencies, and timeout.
- **Parameters**: `path` (string required), `source` (string required), `version` (string, defaults "0.1.0"), `summary` (string), `depends_on` (string[]), `timeout_ms` (integer, defaults 30000)
- **Response**: `{"status":"created", "path":"...", "hash":"...", "version":"..."}`
- **Example**: `{"path":"demo.hello","source":"fn main(){...}","summary":"Hello module"}` -> `{"status":"created","path":"demo.hello","hash":"abc123def456","version":"0.1.0"}`
- **Notes**: Creates a graph node of kind Module. The source is content-addressed using BLAKE3 hashing. The depends_on field references other module paths that must be present. Returns error if the module path is already registered.

### module_build
Compile a registered module into a binary using the Rust compiler. The binary is stored in the build cache for later execution via module_run.
- **Parameters**: `path` (string required), `mode` (string, e.g. "debug" or "release")
- **Response**: `{"status":"built", "path":"...", "hash":"..."}`
- **Example**: `{"path":"demo.hello"}` -> `{"status":"built","path":"demo.hello","hash":"build_hash_xyz"}`
- **Notes**: Requires prior module_create. Uses an incremental build cache to avoid recompilation. On failure returns detailed Rust compiler error messages. The binary path follows the pattern {build_cache_dir}/{path_with_underscores}.

### module_validate
Validate module manifest and source without persisting anything to the registry or graph. Useful for checking module definitions before creating them.
- **Parameters**: `path` (string required), `source` (string required), `version` (string), `summary` (string), `depends_on` (string[]), `timeout_ms` (integer)
- **Response**: `{"valid":true, "path":"..."}`
- **Example**: `{"path":"demo.hello","source":"fn main(){}"}` -> `{"valid":true,"path":"demo.hello"}`
- **Notes**: Errors on empty path or source. Uses the same ModuleCreateParams struct as module_create but is read-only -- no data is persisted. Returns valid=true if the path and source are non-empty.

### module_run
Execute a previously built module as a child process with optional JSON input. The module receives input via stdin or command-line argument.
- **Parameters**: `path` (string required), `input` (object, defaults `{}`)
- **Response**: `{"status":"completed", "output":{...}}`
- **Example**: `{"path":"demo.hello","input":{"name":"world"}}` -> `{"status":"completed","output":{"status":"ok","module":"demo_hello"}}`
- **Notes**: Fails with "not built yet" if no binary exists in the build cache. Input defaults to `{}` when not provided. Uses a 30-second execution timeout. The module output is parsed from stdout as JSON. Runtime working directory is the configured work_dir.

### module_deprecate
Remove a module from the registry and delete its corresponding graph node. This is the reverse of module_create.
- **Parameters**: `path` (string required)
- **Response**: `{"status":"deprecated", "path":"..."}`
- **Example**: `{"path":"demo.hello"}` -> `{"status":"deprecated","path":"demo.hello"}`
- **Notes**: Removes the graph node for this module. Does not delete source or build artifacts from disk. Idempotent -- calling deprecate on a non-existent module still succeeds.

### module_search
Search registered modules by path substring. Returns matching modules with their version, content hash, and build status.
- **Parameters**: `query` (string required, substring to search for in module paths), `limit` (integer, max results, defaults to 20)
- **Response**: `{"count":N, "results":[{"path":"...","version":"...","hash":"...","built":bool}]}`
- **Example**: `{"query":"demo","limit":10}` -> `{"count":1,"results":[{"path":"demo.hello","version":"0.1.0","hash":"abc","built":true}]}`
- **Notes**: Performs simple substring matching on module paths (not full-text search). The results include version, content hash, and whether the module has been built. The built flag is useful for determining if module_run will succeed.

### module_template
Scaffold a new module from a built-in template pattern. Automatically generates Rust source code and registers the module in one step.
- **Parameters**: `path` (string required, module path for the new module), `pattern` (string required, template name), `description` (string, optional summary for the manifest)
- **Response**: `{"status":"created", "path":"...", "pattern":"...", "source_len":N}`
- **Example**: `{"path":"myapp.health","pattern":"health-check","description":"Ping internal endpoints"}` -> `{"status":"created","path":"myapp.health","pattern":"health-check","source_len":2345}`
- **Notes**: Available templates: echo, http-fetch, http-server, db-query, slack-notify, data-transform, health-check, rate-limiter, file-watch, cron-worker. If the pattern is not found, falls back to a generic main() stub. The generated source length is returned in source_len for informational purposes.

### template_list
List all available module templates with their names and descriptions. Use this to discover which patterns can be passed to module_template.
- **Parameters**: None
- **Response**: `{"templates":[{"name":"...","description":"..."}], "count":N}`
- **Example**: `{}` -> `{"templates":[{"name":"echo","description":"Simple JSON echo module"},{"name":"http-fetch","description":"HTTP client module"},{"name":"health-check","description":"Health check module"}],"count":10}`
- **Notes**: Internally registered as module_list_templates. Returns all 10 built-in templates covering common patterns: echo, http-fetch, http-server, db-query, slack-notify, data-transform, health-check, rate-limiter, file-watch, cron-worker.

## Workflows (2 tools)

Workflow tools plan execution graphs by analyzing module dependency chains. The planner traverses depends_on edges transitively starting from a given module and generates a run graph describing the execution plan. The materializer converts the plan into a Directed Acyclic Graph (DAG) using petgraph for cycle detection via topological sort.

The two-step process (plan then materialize) allows agents to inspect the execution plan before committing to execution. Use workflow_plan to see what would be executed, then workflow_materialize to validate the DAG is well-formed. For actual execution, use flow_execute which combines planning, materialization, and execution in a single call.

### workflow_plan
Plan a workflow by traversing module dependencies from a starting module. Generates a run graph that describes the execution plan without running anything.
- **Parameters**: `start` (string required, starting module path), `max_depth` (integer, max dependency depth, defaults to 10)
- **Response**: `{"run_graph_id":"...", "workflow":"...", "modules":N}`
- **Example**: `{"start":"github.issue_triage","max_depth":5}` -> `{"run_graph_id":"rg_abc","workflow":"github.issue_triage","modules":3}`
- **Notes**: The planner resolves depends_on edges transitively up to max_depth. Does not execute any modules. The returned run_graph_id can be passed to materialization or execute flows.

### workflow_materialize
Validate a workflow plan by dry-running the materialization into a DAG without executing.
- **Parameters**: `start` (string required, starting module path), `max_depth` (integer, defaults to 10)
- **Response**: `{"status":"valid_dag"}` on success, or `{"error":"Invalid DAG: ..."}` on failure
- **Example**: `{"start":"github.issue_triage"}` -> `{"status":"valid_dag"}`
- **Notes**: Runs the planner with dry_run=true, then attempts to materialize the plan into a DAG. Detects cycles (via petgraph toposort), missing modules, and invalid dependency references. Use before flow_execute to validate the plan.

## Graph (6 tools)

Graph tools query and manipulate the persistent property graph that tracks all entities and their relationships. Nodes represent modules, workflows, triggers, resources, and runs. Edges represent labeled relationships: DEPENDS_ON, CALLS, TRIGGERS, USES_RESOURCE, EMITS, CONSUMES, BLOCKED_BY, ALTERNATIVE_TO, UPGRADES, DERIVED_FROM.

The graph is backed by SQLite and supports time-range queries, text-based node search, property filtering, and pathfinding between arbitrary nodes. Graph nodes are created implicitly by module_create, flow_create, and flow_execute.

### graph_query
Query graph nodes with optional filtering by kind (module, workflow, trigger, resource), pagination (limit/offset), and property key-value matching.
- **Parameters**: `kind` (string: module|workflow|trigger|resource), `limit` (integer, max results to return), `offset` (integer, pagination offset), `properties` (object, key-value filters for node properties)
- **Response**: `{"count":N, "nodes":[{...}]}`
- **Example**: `{"kind":"module","limit":10,"properties":{"language":"rust"}}` -> `{"count":2,"nodes":[{"name":"demo.hello","kind":"Module","properties":{"language":"rust"}}]}`
- **Notes**: When property filters are active, all nodes of the matching kind are loaded into memory for in-process JSON matching because SQLite stores properties as opaque TEXT. Without property filters, uses efficient paginated queries. Valid kind values: module, workflow, trigger, resource. An empty string or omitted kind returns all node types.

### graph_pathfind
Find all paths connecting two graph nodes using the graph database's pathfinding algorithm. Useful for understanding dependency chains and relationship routes.
- **Parameters**: `from` (string required, source node name), `to` (string required, target node name)
- **Response**: `{"paths_found":N, "paths":[[{"node":{...},"edge":null|{...}},...]]}`
- **Example**: `{"from":"mod_a","to":"mod_c"}` -> `{"paths_found":1,"paths":[[{"node":{"name":"mod_a"},"edge":null},{"node":{"name":"mod_b"},"edge":{"kind":"DEPENDS_ON"}},{"node":{"name":"mod_c"},"edge":{"kind":"DEPENDS_ON"}}]]}`
- **Notes**: Returns paths as arrays of NodeAndEdge objects. Each path alternates between a node entry and an edge entry, starting with the source node (edge: null) and ending with the target node. Can return multiple distinct paths if they exist.

### graph_add_edge
Add a labeled, directed edge between two existing graph nodes. Edges represent relationships like dependency, triggering, resource usage, or data flow.
- **Parameters**: `source` (string required, source node name), `target` (string required, target node name), `kind` (string required, edge relationship type, case-insensitive)
- **Response**: `{"id":"edge_...", "description":"Edge ID string"}`
- **Example**: `{"source":"mod_a","target":"mod_b","kind":"DEPENDS_ON"}` -> `{"id":"edge_xyz"}`
- **Notes**: Valid edge kinds (case-insensitive): DEPENDS_ON, CALLS, TRIGGERS, USES_RESOURCE, EMITS, CONSUMES, BLOCKED_BY, ALTERNATIVE_TO, UPGRADES, DERIVED_FROM. Both source and target nodes must already exist in the graph. Edges are directional.

### graph_summarize
Get aggregated graph statistics broken down by node kind (Module, Workflow, Trigger, Resource, Run) and edge relationship type. Takes no parameters.
- **Parameters**: None
- **Response**: `{"total_nodes":N, "total_edges":N, "nodes_by_kind":{...}, "edges_by_kind":{...}}`
- **Example**: `{}` -> `{"total_nodes":42,"total_edges":67,"nodes_by_kind":{"Module":30,"Workflow":5,"Trigger":3,"Resource":4},"edges_by_kind":{"DEPENDS_ON":40,"TRIGGERS":15,"USES_RESOURCE":12}}`
- **Notes**: High-level overview of the entire graph state. The nodes_by_kind map shows counts per node kind. The edges_by_kind map shows counts per edge relationship type. Useful for monitoring workspace growth and debugging graph structure. Returns the result of the graph's summarize() function.

### graph_search
Search graph nodes by name using SQL LIKE. Useful for finding nodes when you know part of the name but not the full path.
- **Parameters**: `query` (string required)
- **Response**: `{"count":N, "nodes":[{...}]}`
- **Example**: `{"query":"triage"}` -> `{"count":2,"nodes":[{"name":"github.issue_triage","kind":"Module"},{"name":"email.triage_report","kind":"Module"}]}`
- **Notes**: Uses SQL LIKE syntax with % wildcards (not full-text search). Matches substrings within node names. Returns all matching nodes with their full properties including id, kind, name, and user-defined properties.

### graph_time_range
Query nodes and edges created within a specific time range. Useful for auditing, finding recently created resources, or understanding system activity over time.
- **Parameters**: `start` (string required, ISO 8601 datetime), `end` (string required, ISO 8601 datetime), `kind` (string, optional filter by node kind)
- **Response**: `{"nodes":[{...}], "edges":[{...}]}`
- **Example**: `{"start":"2025-01-01T00:00:00Z","end":"2025-12-31T23:59:59Z"}` -> `{"nodes":[{"name":"demo.hello","kind":"Module","created_at":"2025-06-15T10:30:00Z"}],"edges":[]}`
- **Notes**: Both start and end are required and must be valid ISO 8601 strings (e.g. "2025-01-01T00:00:00Z"). Returns both nodes and edges in a single response. The kind parameter filters results to a specific node type.

## Flows (6 tools)

Flow tools compose, inspect, execute, and manage multi-step automation flows. A flow is a sequence of steps, each referencing a registered module by its script_path. Steps execute in order with support for failure policies (abort or continue).

Flows can execute in two modes: as a persisted flow definition (created via flow_create) or as an engine DAG (planned from the module graph). The telemetry variant (flow_execute_telemetry) returns detailed per-step timing, status, retry attempts, and error information in addition to step results. Standard flow_execute returns only step outputs.

### flow_create
Compose and persist a multi-step flow definition. Each step references a registered module by its script_path. The flow is validated (flattened) before persistence.
- **Parameters**: `path` (string required, flow path like "deploy.pipeline"), `steps` (array required, array of FlowStep objects with script_path and kind), `summary` (string, optional description), `on_failure` (string, failure policy like "abort" or "continue")
- **Response**: `{"status":"flow_created", "flow_id":"...", "path":"...", "steps":N}`
- **Example**: `{"path":"deploy.pipeline","steps":[{"script_path":"build.app","kind":"Build"},{"script_path":"test.app","kind":"Test"}],"summary":"CI/CD pipeline","on_failure":"abort"}` -> `{"status":"flow_created","flow_id":"flow_abc","path":"deploy.pipeline","steps":2}`
- **Notes**: Steps are validated by FlowEngine::flatten which checks for structural issues. The flow definition is serialized to JSON and stored in the backend. Each step's script_path should correspond to a registered module path. Steps reference modules by path and execute in sequence.

### flow_show
Retrieve a stored flow definition by path. Returns the full flow record including version, definition (with steps), and summary.
- **Parameters**: `path` (string required, flow path to retrieve)
- **Response**: Full flow object with path, version, definition (steps), summary, and on_failure
- **Example**: `{"path":"deploy.pipeline"}` -> `{"path":"deploy.pipeline","version":"0.1.0","definition":{"steps":[{"script_path":"build.app","kind":"Build"}]},"summary":"CI/CD pipeline"}`
- **Notes**: Returns the full stored flow record from the backend. Returns error "Flow not found" if no flow exists at the given path. Use flow_create to define a new flow first.

### flow_execute
Execute a flow by path. Supports two modes: persisted flow execution (preferred) and DAG fallback (module-based). Creates a Run graph node capturing execution metadata including start time, status, and result count.
- **Parameters**: `path` (string required), `input` (object, JSON input passed to the flow)
- **Response**: `{"status":"completed", "mode":"flow"|"dag", "results":{...}, "run_id":"..." (present in dag mode)}`
- **Example**: `{"path":"deploy.pipeline","input":{"version":"1.0"}}` -> `{"status":"completed","mode":"flow","results":{"build.app":{"status":"ok"},"test.app":{"status":"ok"}}}`
- **Notes**: First tries to load a persisted flow definition from the backend. If no flow is found, falls back to engine DAG execution by planning and executing from the module graph. Creates a graph node of kind Run with execution time, status, and result count. The dag mode response includes a run_id field. Returns step outputs only with no timing telemetry.

### flow_execute_telemetry
Execute a flow and return both step results and detailed Vec<StepTelemetry> with timing, retry attempts, and error information for each step.
- **Parameters**: `path` (string required), `input` (object), `progress_token` (string, optional MCP progress notification token)
- **Response**: `{"status":"completed", "results":{...}, "telemetry":[{"step_id":"...","step_kind":"...","status":"...","started_at":"...","completed_at":"...","duration_ms":N,"retry_attempt":0,"output":{...},"error":null}]}`
- **Example**: `{"path":"deploy.pipeline"}` -> `{"status":"completed","results":{"build.app":{"status":"ok"}},"telemetry":[{"step_id":"build.app","step_kind":"Build","status":"Completed","started_at":"2025-05-07T10:00:00Z","completed_at":"2025-05-07T10:00:05Z","duration_ms":5234,"retry_attempt":0,"output":{"status":"ok"},"error":null}]}`
- **Notes**: The key difference from flow_execute: this tool uses FlowEngine::execute_with_telemetry which returns both step results and telemetry data. When a progress_token is provided, the server sends MCP progress notifications as steps complete. Does not support DAG fallback. Requires a persisted flow definition.

### flow_list
List all stored flow definitions with their paths, versions, and summaries.
- **Parameters**: None
- **Response**: `{"flows":[{"path":"...","version":"...","summary":"..."}]}`
- **Example**: `{}` -> `{"flows":[{"path":"deploy.pipeline","version":"0.1.0","summary":"CI/CD pipeline"},{"path":"nightly.backup","version":"0.1.0","summary":"Nightly database backup"}]}`
- **Notes**: Returns all flows from the backend store. Currently no pagination is implemented. Use flow_show to get the full definition of a specific flow.

### flow_delete
Delete a stored flow definition by path. The flow is removed from the backend store.
- **Parameters**: `path` (string required, flow path to delete)
- **Response**: `{"status":"deleted", "path":"..."}`
- **Example**: `{"path":"deploy.pipeline"}` -> `{"status":"deleted","path":"deploy.pipeline"}`
- **Notes**: Removes the flow definition from the backend store. Does not affect any registered modules, graph nodes, or other resources. The flow path can be reused to create a new flow after deletion.

## Schedules (2 tools)

Schedule tools manage cron-based execution triggers that invoke modules or flows on a timer. The scheduler validates cron expressions before persisting them. Schedules are stored as triggers with type "cron" in the backend and evaluated by the scheduler component at runtime.

Use schedule_validate to check expressions before creating a schedule. The schedule_create tool combines validation and persistence in one step. Cron expressions use standard 5-field format: minute, hour, day of month, month, day of week.

### schedule_create
Create a cron schedule that triggers a module or flow on a timer. The schedule expression is validated before persisting and stored as a trigger in the backend.
- **Parameters**: `target_path` (string required, module or flow path to trigger), `schedule` (string required, standard cron expression like "0 2 * * *"), `args` (object, optional JSON arguments passed to the target on each trigger)
- **Response**: `{"status":"schedule_created", "id":"...", "target":"...", "schedule":"...", "valid_cron":true}`
- **Example**: `{"target_path":"backup.db","schedule":"0 2 * * *","args":{"db":"prod"}}` -> `{"status":"schedule_created","id":"trigger_abc","target":"backup.db","schedule":"0 2 * * *","valid_cron":true}`
- **Notes**: Validates the cron expression using the Scheduler::validate function before persisting. If the expression is invalid, returns an error describing the issue. Stores the schedule as a trigger with type "cron" and the full config (schedule + args) in the backend. The trigger ID can be used for future reference.

### schedule_validate
Validate a cron expression string without creating any schedule. Safe to call repeatedly to check expressions.
- **Parameters**: `schedule` (string required, cron expression to validate)
- **Response**: `{"valid":true, "schedule":"..."}` on success. On failure, returns an error describing the validation issue.
- **Example**: `{"schedule":"*/5 * * * *"}` -> `{"valid":true,"schedule":"*/5 * * * *"}`
- **Notes**: Uses the same Scheduler::validate function used internally by schedule_create. Accepts standard 5-field cron syntax (minute, hour, day of month, month, day of week). Returns a descriptive error message for invalid expressions, such as out-of-range values or syntax errors. No data is persisted.

## Secrets (2 tools)

Secrets tools manage encrypted credentials for API tokens, database passwords, and other sensitive values. Secrets are stored encrypted at rest in the backend store and retrieved by path. They support two operations: secret_set to create or overwrite a secret, and secret_get to retrieve a secret by path.

Secrets are encrypted before storage and decrypted on retrieval. There is no list operation for security reasons -- the caller must know the exact path. Use namespaced paths like "github.token" or "production.db.password" to organize secrets.

### secret_set
Store an encrypted secret value at the given path. If a secret already exists at this path, it is overwritten.
- **Parameters**: `path` (string required, secret path like "github.token"), `value` (string required, the secret value to store), `description` (string, optional human-readable description)
- **Response**: `{"status":"stored", "path":"..."}`
- **Example**: `{"path":"github.token","value":"ghp_xxx","description":"GitHub PAT for API access"}` -> `{"status":"stored","path":"github.token"}`
- **Notes**: Secrets are encrypted at rest in the backend store. Calling secret_set on an existing path overwrites the previous value. There is no way to list secret paths for security reasons.

### secret_get
Retrieve a stored secret value by path. The value is returned in plaintext, so ensure the requesting context is trusted.
- **Parameters**: `path` (string required, secret path to retrieve)
- **Response**: `{"path":"...", "value":"...", "status":"found"}`
- **Example**: `{"path":"github.token"}` -> `{"path":"github.token","value":"ghp_xxx","status":"found"}`
- **Notes**: Returns error "Secret not found" if no value exists at the given path. The secret is decrypted and returned as a string. Use secret_set to create or update secrets.

## Registry (6 tools)

Registry tools manage external resource bindings, background job queues, execution run history, and module search. The registry is a SQLite or Postgres-backed catalog that stores module sources, build artifacts, run records, webhook registrations, and resource configurations. It provides the persistence layer for all Automaton data.

Resource tools (resource_bind, resource_list) let you attach typed external configurations (database URLs, API keys, webhook endpoints) to named paths. Job tools (job_queue, job_list) provide asynchronous execution. Run tools (run_logs, run_retry) enable inspecting and retrying past executions. Registry search (registry_search) finds modules by name.

### resource_bind
Bind a typed external resource configuration (database, API, etc.) to a path. Resources provide runtime configuration like connection strings and API endpoints.
- **Parameters**: `path` (string required), `resource_type` (string required), `value` (object required, resource configuration as JSON)
- **Response**: `{"status":"bound", "path":"...", "type":"..."}`
- **Example**: `{"path":"myapp.db","resource_type":"postgresql","value":{"host":"localhost","port":5432,"database":"mydb"}}` -> `{"status":"bound","path":"myapp.db","type":"postgresql"}`
- **Notes**: Supported resource types: postgresql, slack, github, openai, http, aws. The value field is an arbitrary JSON object whose structure depends on the resource type. Resources can be referenced by modules via the resources field in their manifest.

### resource_list
List all bound resources and the set of supported resource types.
- **Parameters**: None
- **Response**: `{"types":["postgresql","slack","github","openai","http","aws"], "resources":[{...}]}`
- **Example**: `{}` -> `{"types":["postgresql","slack","github","openai","http","aws"],"resources":[{"path":"myapp.db","type":"postgresql","value":{"host":"localhost"}}]}`
- **Notes**: Returns both the full list of supported resource types and all currently bound resources with their paths, types, and values.

### job_queue
Enqueue a background job for asynchronous execution. Jobs are processed by the backend job runner and their status can be tracked via job_list.
- **Parameters**: `target_path` (string required, module path to execute), `args` (object, JSON arguments for the job), `kind` (string, job kind, defaults to "script")
- **Response**: `{"status":"queued", "target":"...", "job_id":"..."}`
- **Example**: `{"target_path":"backup.db","args":{"full":true},"kind":"script"}` -> `{"status":"queued","target":"backup.db","job_id":"job_xyz"}`
- **Notes**: Enqueues the job to the backend job queue. The job runner (a separate component) picks up pending jobs and executes them asynchronously. The returned job_id can be used to track execution status and results via job_list. Jobs are processed in FIFO order.

### job_list
List queued, running, and recently completed jobs with their current status and metadata.
- **Parameters**: None (returns up to 50 most recent jobs)
- **Response**: `{"jobs":[{"id":"...","target":"...","status":"queued|running|completed|failed","created_at":"..."}]}`
- **Example**: `{}` -> `{"jobs":[{"id":"job_abc","target":"backup.db","status":"queued","created_at":"2025-05-07T10:00:00Z"},{"id":"job_def","target":"report.gen","status":"running","created_at":"2025-05-07T09:55:00Z"}]}`
- **Notes**: Returns up to 50 recent jobs from the backend. Each job record includes the job ID, target path, current status (queued, running, completed, or failed), and timestamps. The list includes jobs at various stages of their lifecycle.

### run_logs
Get execution run history for a specific module. Returns past execution records with status and timing information.
- **Parameters**: `module_path` (string, optional filter by module path), `limit` (integer, defaults to 20)
- **Response**: `{"count":N, "runs":[{"id":"...","status":"...","started_at":"..."}]}`
- **Example**: `{"module_path":"backup.db","limit":10}` -> `{"count":2,"runs":[{"id":"run_1","status":"completed","started_at":"2025-05-07T10:00:00Z"},{"id":"run_2","status":"failed","started_at":"2025-05-07T09:00:00Z"}]}`
- **Notes**: If module_path is omitted, returns an empty result set. Runs are returned with the most recent first. Each run record includes ID, status, timestamps, and execution details.

### run_retry
Schedule a retry for a previously failed run. The retry is queued for later processing by the job runner.
- **Parameters**: `run_id` (string required, ID of the failed run to retry)
- **Response**: `{"status":"retry_scheduled", "run_id":"..."}`
- **Example**: `{"run_id":"run_failed_123"}` -> `{"status":"retry_scheduled","run_id":"run_failed_123"}`
- **Notes**: Schedules a retry for execution by the job runner. Does not execute immediately. The run_id must reference a previously failed or completed run.

### registry_search
Search registered modules by path substring. Returns raw registry records as arrays rather than structured JSON objects.
- **Parameters**: `query` (string required, substring to match against module paths)
- **Response**: `{"count":N, "modules":[["path","version","hash",built], ...]}`
- **Example**: `{"query":"backup"}` -> `{"count":1,"modules":[["backup.database","0.1.0","abc123",true]]}`
- **Notes**: Similar to module_search but returns raw registry tuples [path, version, hash, built] instead of structured objects. Performs case-sensitive substring matching on module paths. Use module_search for structured results with field names.

## Webhooks (3 tools)

Webhook tools configure outbound HTTP notifications that fire when execution events occur. Each webhook targets a URL and subscribes to a specific event type. When the event fires, Automaton sends an HTTP POST to the target URL with a JSON payload describing the execution result. An optional HMAC-SHA256 secret enables payload signing for verification by the receiving service.

Available event types: FlowCompleted, FlowFailed, StepCompleted, StepFailed, DagCompleted, DagFailed, RunCompleted, BuildCompleted. Use webhook_register to create, webhook_list to inspect, and webhook_delete to remove webhooks.

### webhook_register
Register an outbound webhook that fires on execution events. Automaton sends HTTP POST requests to the target URL with a JSON payload describing the execution result.
- **Parameters**: `url` (string required, target URL for HTTP POST), `event` (string required, event type to subscribe to), `secret` (string, optional HMAC-SHA256 signing secret)
- **Response**: `{"id":"...", "url":"...", "event":"...", "secret":"..."}`
- **Example**: `{"url":"https://hooks.example.com/auto","event":"FlowCompleted","secret":"whsec_abc"}` -> `{"id":"wh_xyz","url":"https://hooks.example.com/auto","event":"FlowCompleted","secret":"whsec_abc"}`
- **Notes**: Valid event types: FlowCompleted, FlowFailed, StepCompleted, StepFailed, DagCompleted, DagFailed, RunCompleted, BuildCompleted. When the event fires, sends an HTTP POST to url with a JSON body containing execution details. If a secret is provided, the payload is signed with HMAC-SHA256 and included in the X-Automaton-Signature header.

### webhook_list
List all registered webhooks with full configuration including target URL, event type, whether a secret is configured, enabled status, and creation timestamp.
- **Parameters**: None (uses an empty params struct WebhookListParams)
- **Response**: `{"webhooks":[{"id":"...","target_url":"...","event":"...","secret":"...","enabled":true,"created_at":"..."}]}`
- **Example**: `{}` -> `{"webhooks":[{"id":"wh_xyz","target_url":"https://hooks.example.com/auto","event":"FlowCompleted","secret":null,"enabled":true,"created_at":"2025-05-07T10:00:00Z"}]}`
- **Notes**: Returns all registered webhooks from the backend. The id field from this response can be passed to webhook_delete. The secret field returns null if no secret was configured during registration.

### webhook_delete
Delete a webhook registration by its unique ID. After deletion, the webhook will no longer receive event notifications.
- **Parameters**: `id` (string required, webhook ID obtained from webhook_list or returned by webhook_register)
- **Response**: `{"deleted":true}`
- **Example**: `{"id":"wh_xyz"}` -> `{"deleted":true}`
- **Notes**: Permanently removes the webhook from the backend store. Future events of the subscribed type will not trigger HTTP POSTs to this webhook's URL. Returns an error if the webhook ID does not exist in the store.

## System (2 tools)

System tools provide health checks and capability discovery. Use these as a first call when connecting to an Automaton MCP server to understand the available tooling, registered modules, graph state, and system version.

### capability_inventory
Discover available capabilities, registered modules, graph statistics, resource types, and total tool count. Use this as the first call when connecting to an unknown Automaton server to understand its configuration.
- **Parameters**: None
- **Response**: `{"tool_count":38, "modules":N, "graph_nodes":N, "graph_edges":N, "resource_types":["postgresql","slack","github","openai","http","aws"]}`
- **Example**: `{}` -> `{"tool_count":38,"modules":12,"graph_nodes":42,"graph_edges":67,"resource_types":["postgresql","slack","github","openai","http","aws"]}`
- **Notes**: Provides a quick overview of the Automaton workspace state. The tool_count reflects the number of MCP tools handled by the server. The modules count shows registered automation modules. graph_nodes and graph_edges show the size of the persistent property graph. resource_types lists all supported external resource connectors. This tool takes no parameters and can be called with an empty JSON object.

### system_health
Check overall system health including the Automaton version, registered module count, and graph node/edge counts. Use for readiness probes, monitoring, and verifying the server is operational.
- **Parameters**: None
- **Response**: `{"status":"healthy", "version":"...", "registry_modules":N, "graph_nodes":N, "graph_edges":N}`
- **Example**: `{}` -> `{"status":"healthy","version":"0.1.0","registry_modules":12,"graph_nodes":42,"graph_edges":67}`
- **Notes**: Returns a status of "healthy" when the server is operational, along with version and resource counts. The version is the CARGO_PKG_VERSION compiled into the automaton-mcp binary. Use this as a liveness check. Like capability_inventory, this tool takes no parameters.

---

### Common Patterns

All tools follow these conventions:

- **Strict parameter validation**: Every parameter struct uses `#[serde(deny_unknown_fields)]`. Sending an unknown field name causes a deserialization error. This prevents AI agent hallucination of incorrect parameters and ensures tool calls are precise.
- **Error format**: Errors return `{"error": "message string"}` as text content via the err_json() helper. The error message is human-readable and describes what went wrong. Common errors include "not found", "already exists", "invalid params", and internal execution failures.
- **Success format**: Success responses return pretty-printed JSON via the ok_json() helper. Responses include status fields and relevant data. All responses are returned as MCP text content blocks.
- **Argument parsing**: All tools use the parse_args() helper which deserializes from the MCP request arguments map. Missing required fields produce clear error messages indicating which field is missing.
- **Pagination**: List and search tools support limit and offset parameters where relevant. Default limits vary by tool (typically 20 for searches, 50 for job lists).
- **Tool naming**: Tool names use snake_case, matching entries in the MCP server's tools list. The name is the primary dispatch key in the call_tool handler.
- **Graph integration**: Many tools (module_create, flow_create, flow_execute) create graph nodes or edges as side effects. The graph is the persistent record of all automation activity and can be queried via graph tools.
- **Idempotency**: Deprecate and delete tools are generally idempotent. Create tools return errors on duplicate paths. Run operations are not idempotent.
- **Side effects**: module_create, flow_create, and schedule_create all persist data to the backend. Build operations compile binaries to the build cache. Run operations execute child processes.
- **Concurrent use**: The server supports concurrent tool calls. Graph and registry operations are thread-safe. Runtime operations are isolated per call.

### Error Handling

When a tool call fails, the response contains an `error` field in the JSON body rather than an MCP protocol-level error. Common error scenarios:

| Error | Cause |
|-------|-------|
| "Module not found" | Module path does not exist in the registry |
| "not built yet" | Module has been created but not built |
| "Flow not found" | Flow path does not exist in the store |
| "Secret not found" | Secret path has no stored value |
| "Invalid params: ..." | Missing required field or unknown field |
| "Invalid DAG: ..." | Workflow plan has cycles or missing deps |
| "Invalid event: ..." | Webhook event name is not recognized |

Errors are returned as JSON text content, not as MCP protocol errors (which would terminate the session). This means agents can inspect and handle errors programmatically.

### Server Info

The MCP server identifies itself with the name "Automaton MCP Server" and the description "AI-agent-native Rust automation substrate." It supports the tools capability and communicates over stdio transport. The server also implements the get_tool method for tool discovery, though the primary discovery mechanism is list_tools.

### Data Flow

A typical tool call from an AI agent follows this path:

1. Agent discovers available tools via list_tools (returns descriptions and JSON schemas)
2. Agent constructs a tool call with the tool name and JSON arguments
3. Server dispatches to the appropriate handler via name matching in call_tool
4. Handler parses arguments using parse_args() with serde deserialization
5. Handler validates inputs and invokes the appropriate engine/backend/graph/runtime function
6. Success response is serialized to pretty-printed JSON via ok_json()
7. Error response is caught via err_json() and returned as `{"error": "..."}` JSON
8. Agent receives the text content and processes the result

### Schema Discovery

Each tool's parameter schema can be retrieved via the MCP get_tool or list_tools methods. The schema is generated at compile time using the schemars crate from the Rust struct definitions. This means the schema always matches the actual parameter struct, preventing drift between documentation and implementation. Tools with no parameters use an empty JSON object `{}` as their schema.
