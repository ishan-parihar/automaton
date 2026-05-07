# Automaton — AI-Native Rust Automation Substrate

**Automaton** is a CLI-based, graph-native automation framework for AI agents to create, compose, and execute modular Rust workflows. It exposes its entire substrate through an MCP server built on the official [rmcp](https://github.com/modelcontextprotocol/rust-sdk) crate.

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

## Quick Start

```bash
# Initialize workspace
automaton init

# Scaffold a module
automaton new github.issue_triage

# Build it
automaton build github.issue_triage

# Run it
automaton run github.issue_triage --input '{"repo": "user/repo"}'

# Plan a workflow from a module
automaton plan github.issue_triage --max-depth 5

# List modules
automaton list

# Show module details
automaton show github.issue_triage

# Inspect the graph
automaton graph nodes
automaton graph path module_a module_b

# Start MCP server (for AI agents)
automaton mcp

# Diagnostics
automaton doctor

# Postgres Migration (Production)
automaton postgres migrate --database-url "postgres://user:pass@host:5432/automaton"
```

## Workspace Layout

```
~/.local/share/automaton/     # Data directory
├── registry.db               # Module catalog (SQLite)
├── graph.db                  # Property graph store (SQLite)
├── builds/                   # Compiled binary cache
├── modules/                  # Uncompiled module sources
├── work/                     # Runtime working directory
└── tmp/                      # Temp execution artifacts
```

## Project Structure

```
crates/
├── automaton-core/           # Shared types: manifests, graph nodes, errors, telemetry
├── automaton-sdk/            # #[automaton] proc macro + prelude
├── automaton-sdk-derive/     # Proc macro implementation
├── automaton-cli/            # CLI binary
├── automaton-engine/         # Planner, DAG materializer, executor (with Parallelism)
├── automaton-registry/       # SQL-backed module + build + run DB (SQLite/Postgres)
├── automaton-graph/          # SQL-backed property graph store
├── automaton-mcp/            # MCP server (rmcp)
└── automaton-runtime/        # Child process runner, retry, timeout, orphan cleanup
```

## Graph Model

Two-layer architecture:

1. **Design Graph** (persistent property graph): Modules, Workflows, Triggers, Resources, Secrets, Capabilities — interconnected via labeled edges (`DEPENDS_ON`, `CALLS`, `TRIGGERS`, `USES_RESOURCE`, etc.)
2. **Run Graph** (materialized DAG for one execution): Compiled from design graph + context, verified acyclic via `petgraph::toposort`.

## MCP Surface (for AI agents)

The MCP server exposes 39 tools across 9 categories, enabling deep substrate control:

| Category | Key Tools | Description |
|---|---|---|
| **Modules** | `create`, `build`, `validate`, `run`, `deprecate` | Life-cycle management of automation units |
| **Workflows** | `plan`, `materialize`, `execute`, `execute_telemetry` | DAG planning and execution with full telemetry |
| **Graph** | `query`, `pathfind`, `add_edge`, `summarize`, `search`, `time_range` | Property graph manipulation and discovery |
| **Registry** | `search`, `list_templates` | Discovery of registered modules |
| **Resources** | `bind`, `list` | Binding typed resources to modules |
| **Runs** | `logs`, `retry` | Inspecting and re-running executions |
| **System** | `health`, `capability_inventory` | System health and tool capability audit |
| **Webhooks** | `register`, `list`, `delete` | Configuring outbound execution notifications |
| **Secrets** | `set`, `get` | Managing sensitive credentials |

## Module Authoring

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
    // Your automation logic
    Ok(Output {
        summary: format!("Triaged issue #{} from {}", input.issue_number, input.repo),
        priority: "medium".to_string(),
    })
}
```

Manifest: `automation.yaml`

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

## Design Decisions

- **Rust-first**: Smallest binary size and runtime memory footprint.
- **Hybrid Storage**: SQLite for local-first, Postgres for production-grade scalability.
- **High-Throughput Engine**: Level-based DAG parallelism with `futures::join_all`.
- **Agent-First UX**: Dedicated MCP tools for telemetry, graph search, and progress notifications.
- **Resilient Execution**: Process group management (kill_on_drop) to prevent orphan shells.
- **Strict Typing**: `deny_unknown_fields` on all MCP parameter structs to prevent AI hallucinations.
- **Incremental compilation**: Shared build cache, debug/release mode split.

## License

MIT
