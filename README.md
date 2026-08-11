<!-- HERO-IMAGE (T2I spec): 16:9 cinematic hero — a glowing automation DAG (modules as hex nodes, labeled edges DEPENDS_ON/CALLS/TRIGGERS) being assembled by an AI-agent hand on the left, a Windmill-class gear-core at center-right, thin cron-tick rings orbiting. Dark graphite #0F1115 → indigo, single electric-cyan accent. No text except "automaton" watermark bottom-right. Style: dark-tech isometric graph, high contrast, depth-of-field. -->

<p align="center">
  <img src="assets/readme/hero.svg" alt="automaton — AI-Native Graph Orchestration" width="100%">
</p>

# automaton ⚙️

**The Windmill-class, graph-native automation substrate built for AI agents — not for humans.**

[![Rust 1.75+](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
![LOC](https://img.shields.io/badge/LOC-11.7K-informational?style=flat-square)
[![CI](https://github.com/ishan-parihar/automaton/actions/workflows/ci.yml/badge.svg)](https://github.com/ishan-parihar/automaton/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.2.0-orange)](https://github.com/ishan-parihar/automaton/releases)
[![Static Binary](https://img.shields.io/badge/build-static--musl-purple)](https://github.com/ishan-parihar/automaton/releases)
[![MCP](https://img.shields.io/badge/MCP-38%20tools-red)](https://modelcontextprotocol.io)

`automaton` is a CLI-based, graph-native automation framework built in Rust, designed specifically for AI agents to create, compose, and execute modular workflows. It transforms automation from a set of fragile scripts into a structured, version-controlled, and observable substrate.

By exposing its entire core through an MCP (Model Context Protocol) server with 39 precision tools, `automaton` allows LLMs to move beyond simple code generation and into the realm of **Autonomous Systems Engineering** — the same graph-level orchestration that Windmill gives human developers, delivered to Claude, Cursor, Codex, and every other agent **as a first-class tool**.

---

## 🆚 Why automaton instead of Windmill, cron, or agent loops?

Tools like **OpenClaw** and **hermes-agent** treat automation as an afterthought (a cron tick firing a prompt). Windmill gives developers graph orchestration — but it's a heavyweight cloud service built for human UIs. `automaton` is the missing layer: **Windmill-grade graph orchestration, agent-native.**

| Capability | **automaton** | Windmill | cron / OpenClaw / hermes-agent |
|---|---|---|---|
| **Graph-level orchestration** (DAG materialization, dependency discovery, topological sort, parallel dispatch) | ✅ Native — Planner → Materializer → DAG executor | ✅ Native | ❌ Linear scripts |
| **First-class AI-agent access** | ✅ **38-tool MCP server** — full substrate control | ⚠️ Human-first web UI; AI features secondary | ✅ Agents *are* the product (but no graph) |
| **Self-healing workflows** (agents detect failure, query graph, rewrite path) | ✅ `graph_query` + `flow_execute` + `run_retry` compose into repair loops | ❌ Manual | ❌ Manual |
| **Deployment** | ⚙️ **Single static musl binary (~13.5 MB)** — runs anywhere, zero runtime deps | 🐘 Heavy stack: Rust service + Postgres + Redis + workers | 📜 Scripts need language runtimes |
| **Storage** | SQLite (local-first) **or** Postgres (scale) — unified SQL layer | Postgres only | Files/DBs per tool |
| **Cron scheduling** | ✅ Cron daemon (croner), validate + create + list via MCP | ✅ | ✅ (that's all they do) |
| **Capability graph** (discover *how* to solve a problem, not just *what* runs) | ✅ Property graph of modules, workflows, triggers, resources, capabilities | ⚠️ Script registry only | ❌ |
| **Runtime** | Static binary, process-group isolation, `kill_on_drop` — no orphan shells | Containers | Shell / node |
| **Cost / control** | Free, self-hosted, fully local | Self-host costs infra; SaaS has per-seat pricing | Free but shallow |

**The one-liner:** Windmill lets *developers* orchestrate scripts as graphs. `automaton` lets *agents* orchestrate modules as graphs — and agents can see, query, and repair the graph themselves.

---

## 🚩 The Problem: The "Scripting Ceiling"

Traditional automation tools suffer from a critical limitation: they are designed for human developers to write scripts. When AI agents attempt to manage these systems, they hit a "Scripting Ceiling":

- **Opaque Execution**: LLMs struggle to track the state of a complex, multi-step script without constant, expensive log-dumping.
- **Fragile Composition**: Combining two scripts often requires manual boilerplate, making modularity difficult to scale.
- **Lack of Structural Awareness**: Agents cannot "see" the dependency graph of their automation; they can only guess based on the code.
- **Deployment Friction**: Python/Node scripts require heavy runtimes, making deployment to edge devices or restricted VPS environments cumbersome.

## 💡 The Solution: A Graph-Native Substrate

`automaton` replaces the "script" with a **Graph-Based Module**.

### The Core Architecture

`AI Agent` → `MCP (38 Tools)` → `Automaton Engine` → `Execution DAG` → `OS/API`

1. **Modular Design**: Every piece of automation is a "Module"—a self-contained, versioned unit with a strict JSON manifest.
2. **Graph-Based Discovery**: Instead of a file list, `automaton` maintains a property graph of capabilities, dependencies, and observations. Agents query the graph to discover *how* to solve a problem.
3. **DAG Execution**: The engine materializes complex logic (branching, loops, parallelism) into an acyclic Directed Acyclic Graph (DAG), ensuring deterministic execution and maximum throughput via level-based parallel dispatch.
4. **Zero-Dependency Runtime**: Compiled to a static `musl` binary (~13.5 MB), ensuring it runs anywhere without a runtime installation.

---

## ✨ Engineering Highlights

### 🛠 Technical Sophistication
- **39-Tool MCP Surface**: A comprehensive API allowing agents to handle the entire lifecycle: `module_create` → `module_build` → `workflow_plan` → `flow_execute`.
- **Dual-Backend Storage**: Seamlessly switches between SQLite (local-first development) and PostgreSQL (production scalability) using a unified SQL layer.
- **High-Concurrency Engine**: Built on `Tokio` and `Futures`, the engine executes independent DAG nodes concurrently, maximizing resource utilization.
- **Hardened Process Management**: Implements `kill_on_drop` and process group isolation to ensure that timeouts or agent crashes never leave orphan shell processes.
- **Cron Scheduler**: Production-grade daemon using `croner` expressions for reliable, scheduled automation — validated and managed through the MCP surface.

### 🏗 Architectural Components
- **The Planner**: Performs dependency discovery and topological sorting to ensure correct execution order.
- **The Materializer**: Converts high-level flow definitions (Branching, ForLoops) into a flat, executable DAG.
- **The Registry**: A content-addressed build cache that ensures modules are only recompiled when their source changes.
- **The Scheduler**: A production-grade daemon utilizing cron expressions for reliable, scheduled automation.

---

## 🌌 Potentialities & Future Scope

`automaton` is designed to be the "Kernel" for an Autonomous Enterprise:

- **Self-Healing Workflows**: Agents can detect a `run_failure`, query the `graph` for alternative paths, and autonomously rewrite the workflow to bypass the failure.
- **Cross-Agent Collaboration**: Multiple agents can contribute modules to a shared registry, evolving a collective "Capability Graph" over time.
- **Edge-Native Orchestration**: Deploying the static binary to thousands of IoT devices, managed by a central `automaton-api` cluster.
- **Dynamic Capability Discovery**: Moving toward a system where the agent doesn't just use tools, but *invents* new tools by composing existing modules into a new "Super-Module."

---

## 🚀 Quick Start

### Installation
```bash
# Download the static musl binary
curl -L https://github.com/ishan-parihar/automaton/releases/latest/download/automaton -o automaton
chmod +x automaton
sudo mv automaton /usr/local/bin/
```

### Basic Workflow
```bash
# 1. Initialize the substrate
automaton init

# 2. Create a module (e.g., an issue triager)
automaton new github.issue_triage --pattern echo

# 3. Build and run
automaton build github.issue_triage
automaton run github.issue_triage --input '{"repo": "user/repo"}'

# 4. Connect to an AI Agent via MCP
automaton mcp

# Diagnostics
automaton doctor

# Postgres Migration (Production)
automaton postgres migrate --database-url "postgres://user:pass@host:5432/automaton"
```

---

## 🛠 Tech Stack
- **Language**: Rust (Edition 2021)
- **Async Runtime**: Tokio
- **Graph Engine**: Petgraph
- **Storage**: SQLite / PostgreSQL (sqlx)
- **Protocol**: MCP (Model Context Protocol)
- **Build**: static musl binary

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

15 crates, one purpose: agent-native graph orchestration.

```
crates/
├── automaton-core/           # Shared types: manifests, graph nodes, errors, telemetry
├── automaton-sdk/            # #[automaton] proc macro + prelude
├── automaton-sdk-derive/     # Proc macro implementation
├── automaton-cli/            # CLI binary
├── automaton-engine/         # Planner, DAG materializer, executor (with Parallelism)
├── automaton-registry/       # SQL-backed module + build + run DB (SQLite/Postgres)
├── automaton-graph/          # SQL-backed property graph store
├── automaton-mcp/            # MCP server (rmcp) — 39 tools
├── automaton-runtime/        # Child process runner, retry, timeout, orphan cleanup
├── automaton-scheduler/      # Cron daemon (croner) — validate, create, trigger
├── automaton-db/             # Unified SQL layer (SQLite/Postgres)
├── automaton-postgres/       # Postgres migration + production backend
├── automaton-api/            # HTTP API surface (future: multi-instance cluster)
├── automaton-worker/         # Worker runtime for scheduled/queued jobs
└── automaton-build/          # Build pipeline for module compilation
```

## Graph Model

Two-layer architecture:

1. **Design Graph** (persistent property graph): Modules, Workflows, Triggers, Resources, Secrets, Capabilities — interconnected via labeled edges (`DEPENDS_ON`, `CALLS`, `TRIGGERS`, `USES_RESOURCE`, etc.)
2. **Run Graph** (materialized DAG for one execution): Compiled from design graph + context, verified acyclic via `petgraph::toposort`.

## MCP Surface (for AI agents)

The MCP server exposes **39 tools** across 9 categories, enabling deep substrate control:

| Category | Key Tools | Description |
|---|---|---|
| **Modules** | `module_create`, `module_build`, `module_validate`, `module_run`, `module_deprecate`, `module_search`, `module_template`, `module_list_templates` | Life-cycle management of automation units |
| **Workflows** | `workflow_plan`, `workflow_materialize` | DAG planning and validation |
| **Flows** | `flow_create`, `flow_show`, `flow_execute`, `flow_execute_telemetry`, `flow_list`, `flow_delete` | Compose steps into executable DAGs with full telemetry |
| **Graph** | `graph_query`, `graph_pathfind`, `graph_add_edge`, `graph_search`, `graph_time_range`, `graph_summarize` | Property graph manipulation and discovery |
| **Scheduling** | `schedule_create`, `schedule_validate` | Cron-based scheduling |
| **Registry** | `registry_search` | Discovery of registered modules |
| **Resources** | `resource_bind`, `resource_list` | Binding typed resources to modules |
| **Runs** | `run_logs`, `run_retry`, `job_queue`, `job_list` | Inspecting, re-running and queueing executions |
| **System** | `system_health`, `capability_inventory` | System health and tool capability audit |
| **Webhooks** | `webhook_register`, `webhook_list`, `webhook_delete` | Configuring outbound execution notifications |
| **Secrets** | `secret_set`, `secret_get` | Managing sensitive credentials |

## Why Graph-Native Architecture

Most automation tools treat workflows as linear scripts. automaton treats them as **graphs**. Here is why that matters:

### The Script Problem

A script is a filepath. It has no structure, no dependencies, no discovery. When an AI agent needs to accomplish a task, it must:
1. Guess which script to run
2. Hope the script's inputs match what it has
3. Manually chain outputs to inputs across scripts
4. Have no way to discover alternative paths when a script fails

### The Graph Solution

In automaton, every module is a **node** in a property graph. Edges encode relationships: `DEPENDS_ON`, `CALLS`, `TRIGGERS`, `USES_RESOURCE`. When an agent encounters a problem:

1. It queries the graph: `graph_search("github issue triage")` to discover modules
2. It checks dependencies: `graph_pathfind(from=github.fetch, to=slack.notify)` to find a path
3. If a step fails, it queries for alternatives: `graph_query(node=llm.summarize, edge=ALTERNATIVE)`
4. It composes a new workflow: `workflow_plan(steps=[...])` to create a DAG
5. It executes with telemetry: `flow_execute(id, telemetry=true)` to monitor progress

The graph is persistent. The agent's knowledge accumulates. Each workflow execution adds edges to the capability graph. This is how automation evolves from fragile scripts to a **self-improving capability network**.

### The DAG Execution Model

```
Design Graph (persistent)          Run Graph (materialized)
  Module A --DEPENDS_ON--> Module B     [A] --> [B] --> [D]
  Module B --CALLS--> Module C          [C] --------^
  Module C --TRIGGERS--> Module D
                    |
            Planner + Materializer
            (topological sort, cycle detection)
```

The Planner resolves dependencies. The Materializer flattens branching and loops into a DAG. The Executor runs independent nodes in parallel via `futures::join_all`. This is Windmill-grade orchestration, delivered as a single static binary.

---

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

---

## 🤖 Agent Integration (AXI §7)

Automaton ships an installable AI agent skill that provides ambient context at session start — showing module counts, graph stats, and contextual help hints.

```bash
# Via npx (recommended)
npx skills add ishan-parihar/automaton --skill automaton

# Or download manually
curl -fsSL https://raw.githubusercontent.com/ishan-parihar/automaton/master/SKILL.md \
  -o ~/.agents/skills/automaton/SKILL.md
```

### Session Hook (Claude Code)

Add to `~/.claude/settings.json` or project `.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [{ "type": "command", "command": "automaton" }]
      }
    ]
  }
}
```

At session start, automaton prints a compact dashboard:

```
bin: ~/.local/bin/automaton
description: Graph-native automation framework — 38 MCP tools for AI agents

modules[5]{path,version,built}:
  github.issue_triage,0.1.0,2026-07-15
  ...

graph_stats:
  nodes: 12
  edges: 8
  modules: 5

help[4]:
  Run `automaton show <path>` for module details
  Run `automaton new <path> --pattern echo` to create a module
  Run `automaton build <path>` to compile a module
  Run `automaton doctor` to check system health
```

### Session Hook (Codex)

Add to `~/.codex/hooks.json` or project `.codex/hooks.json`:

```json
{
  "SessionStart": "automaton"
}
```

### Session Hook (OpenCode)

```typescript
// ~/.config/opencode/plugins/automaton.ts
export default {
  name: "automaton",
  onSessionStart: async () => {
    const { execSync } = require("child_process");
    return execSync("automaton").toString();
  },
};
```

---

## 📜 License

MIT © [Ishan Parihar](https://github.com/ishan-parihar)

---

## ☕ Support & Sponsorship

If you find this project useful, consider supporting ongoing development:

[![Sponsor](https://img.shields.io/badge/Sponsor-GitHub%20Sponsors-ea4aaa?style=flat-square&logo=github)](https://github.com/sponsors/ishan-parihar)
[![Donate](https://img.shields.io/badge/Donate-Razorpay-3395FF?style=flat-square)](https://rzp.io/rzp/ishan-parihar)

Your support funds new features, releases, and infrastructure for the whole ecosystem.
