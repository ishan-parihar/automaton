# automaton ⚙️

**The High-Performance Substrate for AI-Native Automation.**

[![Rust 1.75+](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen)](https://github.com/ishanp/automaton/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.2.0-orange)](https://github.com/ishanp/automaton/releases)
[![Static Binary](https://img.shields.io/badge/build-static--musl-purple)](https://github.com/ishanp/automaton/releases)
[![MCP](https://img.shields.io/badge/MCP-39%20tools-red)](https://modelcontextprotocol.io)

`automaton` is a CLI-based, graph-native automation framework built in Rust, designed specifically for AI agents to create, compose, and execute modular workflows. It transforms automation from a set of fragile scripts into a structured, version-controlled, and observable substrate.

By exposing its entire core through an MCP (Model Context Protocol) server with 39 precision tools, `automaton` allows LLMs to move beyond simple code generation and into the realm of **Autonomous Systems Engineering**.

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
`AI Agent` $\to$ `MCP (39 Tools)` $\to$ `Automaton Engine` $\to$ `Execution DAG` $\to$ `OS/API`

1. **Modular Design**: Every piece of automation is a "Module"—a self-contained, versioned unit with a strict JSON manifest.
2. **Graph-Based Discovery**: Instead of a file list, `automaton` maintains a property graph of capabilities, dependencies, and observations. Agents query the graph to discover *how* to solve a problem.
3. **DAG Execution**: The engine materializes complex logic (branching, loops, parallelism) into an acyclic Directed Acyclic Graph (DAG), ensuring deterministic execution and maximum throughput via level-based parallel dispatch.
4. **Zero-Dependency Runtime**: Compiled to a static `musl` binary (~14 MB), ensuring it runs anywhere without a runtime installation.

---

## ✨ Engineering Highlights

### 🛠 Technical Sophistication
- **39-Tool MCP Surface**: A comprehensive API allowing agents to handle the entire lifecycle: `module_create` $\to$ `module_build` $\to$ `workflow_plan` $\to$ `flow_execute`.
- **Dual-Backend Storage**: Seamlessly switches between SQLite (local-first development) and PostgreSQL (production scalability) using a unified SQL layer.
- **High-Concurrency Engine**: Built on `Tokio` and `Futures`, the engine executes independent DAG nodes concurrently, maximizing resource utilization.
- **Hardened Process Management**: Implements `kill_on_drop` and process group isolation to ensure that timeouts or agent crashes never leave orphan shell processes.

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
```

## 🛠 Tech Stack
- **Language**: Rust (Edition 2021)
- **Async Runtime**: Tokio
- **Graph Engine**: Petgraph
- **Storage**: SQLite / PostgreSQL (sqlx)
- **Protocol**: MCP (Model Context Protocol)
- **Build**: static musl binary

---
Developed by [Ishan Parihar](https://github.com/ishan-parihar) to bridge the gap between LLM reasoning and deterministic system execution.
