---
name: automaton
description: >
  Graph-native automation framework for AI agents. Create, build, and
  execute modular workflows with 39 MCP tools. DAG execution, property
  graph discovery, and production-grade scheduling.
---

# Automaton Skill

Graph-native automation framework — 39 MCP tools for AI agents to create, compose, and execute modular workflows.

<!-- Static skill — regenerate from CLI: automaton --help -->
<!-- Install: npx skills add ishan-parihar/automaton --skill automaton -->
<!-- CI check: diff <(automaton --help) SKILL.md && exit 1 -->

## Quick Start

```bash
# Initialize the substrate
automaton init

# Create a module
automaton new github.issue_triage --pattern echo

# Build and run
automaton build github.issue_triage
automaton run github.issue_triage --input '{"repo": "user/repo"}'

# Start MCP server for AI agents
automaton mcp
```

## MCP Configuration

```json
{
  "mcpServers": {
    "automaton": {
      "command": "automaton",
      "args": ["mcp"]
    }
  }
}
```

## Key Tools (39 total)

| Category | Tools | Description |
|----------|-------|-------------|
| Modules | 5 | create, build, validate, run, deprecate |
| Workflows | 4 | plan, materialize, execute, telemetry |
| Graph | 6 | query, pathfind, add_edge, summarize, search, time_range |
| Registry | 2 | search, list_templates |
| Resources | 2 | bind, list |
| Runs | 2 | logs, retry |
| System | 2 | health, capability_inventory |
| Webhooks | 3 | register, list, delete |
| Secrets | 2 | set, get |
| Additional | 11 | Various utility tools |

## Architecture

```
crates/
├── automaton-core/           # Shared types: manifests, graph nodes, errors
├── automaton-sdk/            # #[automaton] proc macro + prelude
├── automaton-cli/            # CLI binary
├── automaton-engine/         # Planner, DAG materializer, executor
├── automaton-registry/       # SQL-backed module + build + run DB
├── automaton-graph/          # SQL-backed property graph store
├── automaton-mcp/            # MCP server (rmcp)
└── automaton-runtime/        # Child process runner, retry, timeout
```

## Data Directory

```
~/.local/share/automaton/
├── registry.db               # Module catalog (SQLite)
├── graph.db                  # Property graph store (SQLite)
├── builds/                   # Compiled binary cache
├── modules/                  # Uncompiled module sources
└── work/                     # Runtime working directory
```

