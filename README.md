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
│ (SQLite-backed)  │    │                              │
└─────────────────┘    └──────────────────────────────┘
       │                          │
       └──────────┬───────────────┘
                  │
┌─────────────────▼───────────────────────────────────┐
│                automaton-registry                    │
│  SQLite-backed module catalog + content-addressed    │
│  build cache + run history                          │
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
├── automaton-core/           # Shared types: manifests, graph nodes, errors
├── automaton-sdk/            # #[automaton] proc macro + prelude
├── automaton-sdk-derive/     # Proc macro implementation
├── automaton-cli/            # CLI binary
├── automaton-engine/         # Planner, DAG materializer, executor
├── automaton-registry/       # SQLite-backed module + build + run DB
├── automaton-graph/          # SQLite-backed property graph store
├── automaton-mcp/            # MCP server (rmcp)
└── automaton-runtime/        # Child process runner, retry, timeout
```

## Graph Model

Two-layer architecture:

1. **Design Graph** (persistent property graph): Modules, Workflows, Triggers, Resources, Secrets, Capabilities — interconnected via labeled edges (`DEPENDS_ON`, `CALLS`, `TRIGGERS`, `USES_RESOURCE`, etc.)

2. **Run Graph** (materialized DAG for one execution): Compiled from design graph + context, verified acyclic via `petgraph::toposort`.

## MCP Surface (for AI agents)

| Tool                   | Description                            |
| ---------------------- | -------------------------------------- |
| `module.create`        | Register a new automation module       |
| `module.build`         | Compile module to binary               |
| `module.validate`      | Validate manifest + source             |
| `module.run`           | Execute a module                       |
| `module.deprecate`     | Remove module                          |
| `workflow.plan`        | Discover dependencies, build run graph |
| `workflow.materialize` | Verify DAG validity (cycle check)      |
| `graph.query`          | Query nodes by kind                    |
| `graph.pathfind`       | Find paths between nodes               |
| `graph.add_edge`       | Wire module dependencies               |
| `registry.search`      | Search registered modules              |
| `resource.bind`        | Bind typed resources                   |
| `run.logs`             | Inspect run history                    |
| `run.retry`            | Retry failed execution                 |
| `system.health`        | Component health check                 |

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

- **Rust-first**: Smallest binary size (~2.7 MB compiled) and runtime memory (~4 MB RSS)
- **SQLite-backed**: No Postgres dependency for local-first use
- **petgraph DAG**: Topological scheduling with cycle detection
- **Windmill-inspired packaging**: Code + YAML manifest, content-addressed builds
- **MCP-native control**: AI agents interact through the MCP protocol, not raw shell
- **Property graph**: Two-layer (design + run) enables recursive module composition
- **Incremental compilation**: Shared build cache, debug/release mode split

## License

MIT
