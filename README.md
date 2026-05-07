# Automaton — AI-Native Rust Automation Substrate

[![Rust 1.75+](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen)](https://github.com/ishanp/automaton/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.2.0-orange)](https://github.com/ishanp/automaton/releases)
[![Static Binary](https://img.shields.io/badge/build-static--musl-purple)](https://github.com/ishanp/automaton/releases)
[![MCP](https://img.shields.io/badge/MCP-39%20tools-red)](https://modelcontextprotocol.io)

**Automaton** is a CLI-based, graph-native automation framework built in Rust for AI agents to create, compose, and execute modular workflows. It exposes its entire substrate through an MCP (Model Context Protocol) server with 39 tools, backed by a property graph knowledge base, a DAG-based flow engine, and dual SQLite/Postgres storage. Automaton gives LLM agents deep, structured control over automation lifecycles, from module authoring to production scheduling.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    AI Agent (LLM)                        │
│  Uses MCP tools to create modules → plan → execute       │
└──────────────────────┬──────────────────────────────────┘
                       │ MCP (stdio/SSE)
┌──────────────────────▼──────────────────────────────────┐
│                automaton-mcp (MCP Server)                │
│  module.* | workflow.* | graph.* | registry.* | run.*   │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│                automaton-engine (Orchestrator)           │
│  Planner → Materializer → Executor                      │
│  Converts design graph → run DAG → execution            │
└──────┬──────────────────────────┬───────────────────────┘
       │                          │
┌──────▼──────────┐    ┌──────────▼───────────────────┐
│ automaton-graph │    │ automaton-runtime             │
│ Property graph   │    │ Process sandbox, retry, I/O │
│ (SQLite/PG-backed)│    │                              │
└─────────────────┘    └──────────────────────────────┘
       │                          │
       └──────────┬───────────────┘
                  │
┌─────────────────▼───────────────────────────────────┐
│                automaton-registry                    │
│  SQLite/Postgres-backed module catalog + content-addressed│
│  build cache + run history + webhooks                │
└─────────────────────────────────────────────────────┘
```

## Key Features

- **39 MCP Tools** across 9 categories, purpose-built for AI agent control
- **Flow Composition** with Shell, CallFlow, BranchOne, BranchAll, ForLoop, WhileLoop, and FailureModule step kinds, plus level-based DAG parallelism
- **Property Graph Knowledge Base** with 12 NodeKinds, 10 EdgeKinds, `json_extract` SQL queries, text search, and time-range filtering
- **Dual Storage Backend** with SQLite for local-first development and Postgres for production-grade scalability
- **Engine Pipeline** with Plan (dependency discovery), Materialize (acyclic DAG construction), and Execute (level-based parallel dispatch)
- **Production Daemons** with worker and scheduler processes for cron-based execution and job queue processing
- **Resilient Execution** with process group management (`kill_on_drop`), configurable retry (fixed/linear/exponential backoff), and orphan shell cleanup
- **Rate-Limited REST API** built on axum with tower middleware
- **Static musl Binary** (~14 MB, x86_64-unknown-linux-musl) for zero-dependency deployment
- **Webhook System** for outbound execution notifications on completion or failure
- **Secrets Management** with AES-256-GCM encrypted storage
- **Content-Addressed Build Cache** for incremental module compilation

## Quick Start

```bash
# Initialize workspace
automaton init

# Scaffold a module from a template
automaton new github.issue_triage --pattern echo

# Build it
automaton build github.issue_triage

# Run it
automaton run github.issue_triage --input '{"repo": "user/repo"}'

# Plan a workflow from a module
automaton plan github.issue_triage --max-depth 5

# Execute a workflow
automaton execute github.issue_triage --max-depth 5 --input '{"repo": "user/repo"}'

# List registered modules
automaton list

# Show module details
automaton show github.issue_triage

# Inspect the property graph
automaton graph nodes
automaton graph path module_a module_b

# View run logs
automaton logs --limit 20

# Start the MCP server (for AI agents)
automaton mcp

# Start the worker daemon
automaton worker --concurrency 4 --daemon

# Run system diagnostics
automaton doctor

# Postgres migration (production)
automaton postgres migrate --database-url "postgres://user:pass@host:5432/automaton"
```

## Why Automaton?

- **Designed for AI agents, not just humans.** Every capability is surfaced as an MCP tool with strict JSON schema validation (`deny_unknown_fields` prevents hallucinated parameters). AI agents can create modules, query the graph, compose flows, inspect execution telemetry, and manage secrets through the same interface.
- **Agent-first UX with MCP progress notifications.** The `flow_execute_telemetry` tool streams per-step progress back to the agent, enabling live status updates and informed decision-making during long-running workflows.
- **Composition is a first-class primitive.** Shell commands, branching (pick-one or run-all), for-loops, while-loops, and cross-flow calls can be nested arbitrarily. Flatten a complex flow into an executable DAG in a single call.
- **Graph-native knowledge persistence.** The property graph stores not just module metadata but observations, constraints, alternatives, and temporal relationships. Agents discover capabilities through graph search and pathfinding rather than static documentation.

## Workspace Layout

```
~/.local/share/automaton/     # Data directory
├── registry.db               # Module catalog, flows, jobs, webhooks (SQLite/Postgres)
├── builds/                   # Compiled binary cache
├── modules/                  # Uncompiled module sources
├── work/                     # Runtime working directory
└── tmp/                      # Temp execution artifacts
```

## Project Structure (15 Crates)

```
crates/
├── automaton-core/           # Shared types: manifests, graph nodes, flow steps, errors, telemetry
├── automaton-sdk/            # #[automaton] proc macro + prelude for module authoring
├── automaton-sdk-derive/     # Proc macro implementation
├── automaton-cli/            # CLI binary (init, new, build, run, plan, execute, mcp, worker, etc.)
├── automaton-engine/         # Planner, DAG materializer, executor (level-based parallelism)
├── automaton-flow/           # Flow engine: branch, loop, call-flow, failure step execution
├── automaton-registry/       # SQL/Postgres-backed module catalog, build cache, run history
├── automaton-graph/          # SQL-backed property graph store with json_extract queries
├── automaton-mcp/            # MCP server (rmcp) exposing 39 tools
├── automaton-api/            # REST API server (axum) with rate limiting
├── automaton-runtime/        # Child process runner, retry, timeout, orphan cleanup
├── automaton-build/          # Module compilation, content-addressed build cache
├── automaton-worker/         # Queue-based job worker daemon
├── automaton-scheduler/      # Cron expression parser and trigger scheduler
├── automaton-postgres/       # Postgres-specific schema migration and connection pool
```

## MCP Tool Surface (39 Tools)

All 39 tools are exposed through the MCP server and usable by any MCP-compatible AI agent. Each tool accepts typed JSON parameters with `deny_unknown_fields` enforcement.

### Modules (8)

| Tool | Description |
|---|---|
| `module_create` | Register a new automation module with source code and manifest |
| `module_build` | Compile a registered module into a cached binary |
| `module_validate` | Validate module manifest and source |
| `module_run` | Execute a compiled module with JSON input |
| `module_deprecate` | Remove a module from the registry and graph |
| `module_search` | Search modules by name query |
| `module_template` | Generate module scaffolding from a named template |
| `module_list_templates` | List available scaffolding templates |

### Workflows (2)

| Tool | Description |
|---|---|
| `workflow_plan` | Discover dependency graph from a module and produce a run graph |
| `workflow_materialize` | Validate a planned DAG for acyclicity without executing |

### Graph (6)

| Tool | Description |
|---|---|
| `graph_query` | Query nodes by kind, pagination, and property filters |
| `graph_pathfind` | Find all paths between two nodes with DFS traversal |
| `graph_add_edge` | Wire a typed edge between two graph nodes |
| `graph_search` | Search nodes by name substring match (LIKE query) |
| `graph_time_range` | Find nodes and edges within an ISO 8601 time range |
| `graph_summarize` | Get aggregated node/edge counts by kind |

### Flows (6)

| Tool | Description |
|---|---|
| `flow_create` | Compose a multi-step flow definition with branching and loops |
| `flow_show` | Retrieve a stored flow definition |
| `flow_execute` | Execute a flow (persisted or module-based DAG) |
| `flow_execute_telemetry` | Execute a flow with per-step timing and progress notifications |
| `flow_list` | List all stored flow definitions |
| `flow_delete` | Remove a stored flow |

### Schedules (2)

| Tool | Description |
|---|---|
| `schedule_create` | Register a cron-triggered schedule for a module or flow |
| `schedule_validate` | Validate a cron expression string |

### Secrets (2)

| Tool | Description |
|---|---|
| `secret_set` | Store an AES-256-GCM encrypted secret |
| `secret_get` | Retrieve a stored secret value |

### Resources (2)

| Tool | Description |
|---|---|
| `resource_bind` | Bind a typed resource configuration to a module |
| `resource_list` | List available resource types (postgresql, slack, github, openai, http, aws) |

### Jobs and Runs (4)

| Tool | Description |
|---|---|
| `job_queue` | Enqueue a job for the worker daemon |
| `job_list` | List queued and completed jobs |
| `run_logs` | Get execution history for a module |
| `run_retry` | Schedule a retry for a failed run |

### Registry (1)

| Tool | Description |
|---|---|
| `registry_search` | Search the module registry by path prefix |

### Webhooks (3)

| Tool | Description |
|---|---|
| `webhook_register` | Register an outbound webhook for execution events |
| `webhook_list` | List all registered webhooks |
| `webhook_delete` | Delete a webhook by ID |

### System (3)

| Tool | Description |
|---|---|
| `capability_inventory` | Discover all available capabilities, modules, and graph statistics |
| `system_health` | Check system version, registry module count, and graph size |

## Graph Model

Two-layer architecture:

1. **Design Graph** (persistent property graph): Modules, Workflows, Triggers, Resources, Secrets, Capabilities, Artifacts, Runs, Observations, Constraints, AlternativePaths, and Inputs interconnected via labeled edges.
2. **Run Graph** (materialized DAG for one execution): Compiled from the design graph plus runtime context, verified acyclic via `petgraph::toposort`, and dispatched in topological levels with `futures::join_all`.

### Node Kinds (12)

`Module`, `Workflow`, `Trigger`, `Resource`, `SecretRef`, `Capability`, `Artifact`, `Run`, `Observation`, `Constraint`, `AlternativePath`, `Input`

### Edge Kinds (10)

`DependsOn`, `Calls`, `Emits`, `Consumes`, `Triggers`, `UsesResource`, `BlockedBy`, `AlternativeTo`, `Upgrades`, `DerivedFrom`

Nodes and edges carry arbitrary JSON properties. Queries use `json_extract()` at the SQL level for efficient server-side filtering, with fallback to in-memory scan for advanced filter combinations.

## Flow Composition

Flows are JSON/YAML structures that compose multiple step kinds into an executable DAG. Steps within the same dependency level execute concurrently.

```yaml
# flow.yaml — social.daily_pipeline
path: social.daily_pipeline
version: 0.1.0
summary: "Daily social media pipeline with conditional branching"
default_timeout_ms: 60000
on_failure: notify.ops_team
steps:
  - id: fetch_posts
    kind: Script
    script_path: social.posts
    input:
      platform: "twitter"
      count: 100

  - id: sentiment_check
    kind: Script
    script_path: nlp.sentiment
    depends_on: [fetch_posts]
    input:
      posts: "${fetch_posts.results}"
    stop_if: "${sentiment_check.alert} == false"

  - id: alert_or_archive
    kind: BranchOne
    depends_on: [sentiment_check]
    branches:
      - - id: send_alert
          kind: Shell
          shell: "bash"
          command: "curl -X POST https://hooks.internal/alert -d '${sentiment_check}'"
          timeout_ms: 10000
      - - id: archive_results
          kind: Script
          script_path: data.archive
          input:
            data: "${sentiment_check}"

  - id: enrich_loop
    kind: ForLoop
    iterator: fetch_posts
    depends_on: [fetch_posts]
    steps:
      - id: enrich_item
        kind: Script
        script_path: enrich.user_data
        input:
          user: "${fetch_posts__item.user_id}"

  - id: notify_done
    kind: CallFlow
    flow_path: notify.slack_message
    depends_on: [alert_or_archive, enrich_loop]
    input:
      channel: "#ops"
      message: "Pipeline completed"
```

The `FlowEngine::flatten()` call resolves nesting (BranchOne, BranchAll, ForLoop, WhileLoop) into an ordered step list with explicit dependencies, which is then executed by the Runtime with full retry, timeout, and state reference resolution (`${step_id.field}`).

## Module Authoring

Modules are Rust crates stamped with the `#[automaton]` attribute macro:

```rust
use automaton_sdk::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
struct Input {
    repo: String,
    issue_number: u32,
}

#[derive(Serialize, JsonSchema)]
struct Output {
    summary: String,
    priority: String,
}

#[automaton]
async fn main(ctx: Context, input: Input) -> anyhow::Result<Output> {
    Ok(Output {
        summary: format!("Triaged issue #{} from {}", input.issue_number, input.repo),
        priority: "medium".to_string(),
    })
}
```

Each module ships with an `automation.yaml` manifest:

```yaml
name: github.issue_triage
version: 0.1.0
entry: main
summary: "Triage GitHub issues by priority"
timeout_ms: 30000
retry:
  max_attempts: 3
  delay_ms: 1000
  backoff: exponential
permissions:
  - github.read
resources:
  - github.api
depends_on:
  - llm.summarize
tags:
  - github
  - issue
  - triage
```

The SDK provides scaffolding templates: `echo`, `http-fetch`, `http-server`, `db-query`, `slack-notify`, `data-transform`, `health-check`, `rate-limiter`, `file-watch`, `cron-worker`.

## Production Deployment

### Static Binary

Build a zero-dependency musl binary for deployment:

```bash
cargo build --release --target x86_64-unknown-linux-musl
# Binary: target/x86_64-unknown-linux-musl/release/automaton (~14 MB)
```

### Postgres Backend

Migrate from SQLite to Postgres for production:

```bash
automaton postgres migrate --database-url "postgres://user:pass@host:5432/automaton"
```

This runs schema migrations on the Postgres instance, creating tables for the registry, graph nodes and edges, flows, jobs, webhooks, and variables.

### Daemon Processes

Two long-running processes support production workflows:

- **Worker** (`automaton worker --concurrency 4`): Polls the job queue and executes enqueued module runs concurrently.
- **Scheduler** (embedded in `automaton start`): Evaluates cron triggers and enqueues scheduled runs on matching schedules.

### Rate Limiting

The REST API (provided by `automaton-api` on axum) includes rate limiting via tower middleware to protect against abuse in multi-tenant deployments.

### Webhooks

Register webhooks for push-based notification on execution events:

```json
{
  "url": "https://hooks.example.com/automaton-events",
  "event": "run.completed",
  "secret": "whsec_..."
}
```

## Design Decisions

- **Rust-first.** Minimal binary size (~14 MB static musl), no runtime dependency, predictable performance.
- **Hybrid storage.** SQLite for local-first development, Postgres for production scale. Same schema, same queries.
- **Level-based DAG parallelism.** Nodes at the same topological level execute concurrently via `futures::join_all`, maximizing throughput without a scheduler thread.
- **Agent-first MUX.** Every MCP tool uses strict `deny_unknown_fields` deserialization to prevent AI hallucinated parameters from causing silent failures.
- **Content-addressed build cache.** Module binaries are indexed by source hash. Rebuilds only compile when the source changes.
- **Cross-step state references.** Use `${module_path.field}` syntax in inputs to reference upstream outputs. Resolved at execution time from the accumulated flow state map.
- **Process group isolation.** Child processes use `kill_on_drop(true)` to eliminate orphan shell processes on timeout or cancellation.

## Documentation Reference

- [Architecture Guide](docs/ARCHITECTURE.md) — Detailed component design and data flow
- [Flows Guide](docs/FLOWS.md) — Flow composition, branching, loops, and CallFlow patterns
- [MCP Tools Reference](docs/MCP_TOOLS.md) — Full parameter schemas for all 39 tools
- [Production Deployment](docs/PRODUCTION.md) — Postgres setup, daemon management, rate limiting, webhooks

## Project Status

**Current version: v0.2.0** — Active development.

- 15 crates with clean separation of concerns
- 39 MCP tools, fully functional
- 71+ tests across all major components
- Postgres backend with migration support
- Static musl binary releases (GitHub + GitLab)
- Worker and scheduler daemons for production
- REST API with axum and rate limiting
- Active issue tracker and roadmap

## License

MIT
