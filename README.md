# Automaton

![Rust](https://img.shields.io/badge/Rust-1.78+-orange?logo=rust)
![Python](https://img.shields.io/badge/Python-3.11+-blue?logo=python)
![License](https://img.shields.io/badge/License-MIT-green)
![MCP](https://img.shields.io/badge/MCP-1.0-orange?logo=modelcontextprotocol)
![Substrate](https://img.shields.io/badge/Substrate-Rust-purple)


**AI-native automation substrate** — Rust substrate, Python DSL, agent orchestrator, built for the agent era.

![Automaton architecture](https://github.com/ishan-parihar/automaton/raw/main/assets/readme/automaton-arch.png)

---

## What it is

| Layer | Technology | Purpose |
|-------|------------|---------|
| **Substrate** | Rust (tokio, sled) | High-perf execution, persistence, scheduling |
| **DSL** | Python (pydantic, jinja2) | Workflow definition, templating, logic |
| **Orchestrator** | Python + MCP | Agent coordination, tool routing, state |

**Philosophy:** Infrastructure as code → Infrastructure as agent.

---

## Quick start

```bash
# Install
cargo install --git https://github.com/ishan-parihar/automaton automaton-cli
pipx install automaton-dsl

# Define workflow (YAML)
cat > workflow.yaml << 'EOF'
name: "daily-research"
trigger: "cron: 0 6 * * *"
steps:
  - name: fetch-papers
    tool: research.search
    args: {query: "LLM agents", limit: 10}
  - name: summarize
    tool: llm.complete
    args: {prompt: "Summarize: {{fetch-papers}}"}
  - name: notify
    tool: telegram.send
    args: {chat_id: "123456", text: "{{summarize}}"}
EOF

# Run
automaton run workflow.yaml
```

---


## DSL Features

| Feature | Example |
|---------|---------|
| **Templating** | `{{step_name.output.field}}` |
| **Conditionals** | `when: "{{fetch.status}} == 'ok'"` |
| **Loops** | `for: "{{items}}"` |
| **Parallel** | `parallel: true` on step group |
| **Retry** | `retry: 3, backoff: exponential` |
| **Secrets** | `${{secrets.API_KEY}}` |

---

## MCP Integration

```yaml
# Automaton workflows call MCP tools directly
steps:
  - name: browse
    tool: mcp.igs.browser_markdown
    args: {url: "https://example.com"}
  - name: extract
    tool: mcp.llm.complete
    args: {prompt: "Extract key points: {{browse}}"}
```

---



## Visual proof

| Workflow DAG | Scheduler | State store |
|:---:|:---:|:---:|
| ![DAG](https://github.com/ishan-parihar/automaton/raw/main/assets/readme/dag.png) | ![Scheduler](https://github.com/ishan-parihar/automaton/raw/main/assets/readme/scheduler.png) | ![State](https://github.com/ishan-parihar/automaton/raw/main/assets/readme/state.png) |

| DSL example | Executor | MCP tools |
|:---:|:---:|:---:|
| ![DSL](https://github.com/ishan-parihar/automaton/raw/main/assets/readme/dsl.png) | ![Executor](https://github.com/ishan-parihar/automaton/raw/main/assets/readme/executor.png) | ![MCP](https://github.com/ishan-parihar/automaton/raw/main/assets/readme/mcp.png) |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Automaton Core                           │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │  Scheduler   │  │  Executor    │  │  State Store │       │
│  │  (cron,      │  │  (DAG,       │  │  (sled,      │       │
│  │   event,     │  │   parallel,  │  │   ACID)      │       │
│  │   manual)    │  │   retry)     │  │              │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
└─────────────────────────────────────────────────────────────┘
         │                │                │
         ▼                ▼                ▼
┌─────────────────────────────────────────────────────────────┐
│  Tools: MCP servers, CLI commands, HTTP APIs, Python fns    │
└─────────────────────────────────────────────────────────────┘
```

---

## Requirements

- Rust 1.78+ (substrate)
- Python 3.11+ (DSL/orchestrator)
- sled (embedded DB, zero-config)

---

## License

MIT — see [LICENSE](LICENSE).
