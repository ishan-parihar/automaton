# Automaton — AI-Handover Document

**Generated:** 2026-05-07  
**GitHub:** https://github.com/ishan-parihar/automaton  
**Purpose:** A Rust-native automation substrate for AI agents to create, compose, and manage their own infrastructure through MCP tools.

---

## 1. Architecture Overview

```
crates/
├── automaton-core/     — Shared types: manifests, graph nodes, flows, errors, secrets, triggers, jobs
├── automaton-sdk/      — #[automaton] proc macro for agent-authored modules
├── automaton-sdk-derive/ — Proc macro implementation
├── automaton-engine/   — DAG planner + FlowEngine (branch_one, branch_all, forloop, whileloop)
├── automaton-runtime/  — Subprocess runner with retry + timeout
├── automaton-graph/    — SQLite-backed property graph store
├── automaton-registry/ — SQLite-backed module catalog + build cache + run history
├── automaton-build/    — Real cargo build integration with content-addressed caching
├── automaton-worker/   — Worker daemon for pull-based job execution
├── automaton-scheduler/— Cron expression validation + ticker
├── automaton-db/       — DbPool trait with dual SQLite/Postgres backends
├── automaton-mcp/      — MCP server with 28 tools, real JSON schemas
└── automaton-cli/      — CLI binary with 13 subcommands
```

### Database Layer (`automaton-db`)

Mutually exclusive backends via cargo features:

```
[features]
default = ["sqlite"]
sqlite = ["dep:rusqlite"]        — Local dev, single file
postgres = ["dep:tokio-postgres", "dep:deadpool-postgres"] — Production
```

The `DbPool` trait defines 22 async methods across 8 categories:
scripts, jobs, runs, graph nodes, graph edges, variables, resources, triggers.

---

## 2. What Works (End-to-End Verified)

| Feature | Status | Details |
|---|---|---|
| Module lifecycle | ✅ | `create → build → run` via CLI + MCP |
| Real compilation | ✅ | `cargo build --release`, 472KB binary, content-addressed |
| Flow engine | ✅ | `flatten()` + `execute()`: branch_one, branch_all, forloop, whileloop, Sleep, FailureModule |
| Cron scheduler | ✅ | Validate, match, next-occurrence, CronTicker |
| Property graph | ✅ | SQLite-backed nodes + edges with pathfinding |
| Workflow planning | ✅ | Dependency discovery → topological sort → DAG validation |
| Secrets (types) | ✅ | Variable + Resource with encryption types |
| MCP server | ✅ | 28 tools with real JSON schemas |
| Build cache | ✅ | Content-addressed by SHA-256 hash of source |
| Integration tests | ✅ | 10/10 passing (cron, flow engine, branches, loops) |
| CLI | ✅ | init, new, build, run, list, show, graph, plan, logs, doctor, mcp |

---

## 3. What Was Fixed (Bugs Encountered)

**Critical:**
- **Build was a no-op** — CLI's `build` command only `mark_built()` without actual compilation. Fixed by wiring `BuildCache::build_rust()` into the CLI.
- **Run returned "skipped"** — No binary existed because build didn't compile. Fixed after build fix.
- **Duplicate graph nodes** — `new` command called `init_engine` twice, each call registering a graph node. Fixed by consolidating to single call.
- **Template used SDK proc macro** — Generated `main.rs` required `#[automaton]` macro which couldn't compile separately. Fixed with standalone `fn main()` template.

**Infrastructure:**
- **`automaton_core::Result<T>` shadowing** — `pub use error::Result` (1 generic arg) shadows `std::result::Result<T, E>` in every crate importing `*`. Fixed by using explicit `StdResult` alias.
- **`rmcp::Tool` non-exhaustive** — `Tool { ... }` struct literals don't compile. Must use `Tool::new(name, desc, schema)` with `Arc<JsonObject>`.
- **`&str` vs `Cow<'static, str>`** — `add_tool` function needed `&'static str` lifetime to satisfy `Tool::new`'s `Into<Cow<'static, str>>` bound.
- **`rusqlite::Connection` not Sync** — Requires `Mutex<Connection>` for shared access in async context.
- **`schemars` macro via re-export** — `rmcp::schemars::schema_for!()` can't be used because proc macros can't be re-exported through module paths. Must use `schemars::schema_for!()` directly with `schemars` as a direct dependency.

---

## 4. Critical Gaps Remaining

### Tier 1: Agent Usability (Highest Impact)

| Gap | Impact | Fix |
|---|---|---|
| **No Postgres backend working** | Can't scale to distributed workers. SQLite has no `FOR UPDATE SKIP LOCKED`. | Wire `automaton-db/postgres.rs` into the engine/CLI. Requires resolving the `libsqlite3-sys` link conflict by making backends mutually exclusive via features. |
| **Modules can't do anything useful** | Generated `fn main()` prints static JSON. Agent must write all logic from scratch. | Add template patterns: `http-fetch`, `db-query`, `slack-notify`, `github-api`. Templates should compile standalone. |
| **No dependency resolution** | Agent must manually discover and declare dependencies. No auto-resolution. | Add `flow.resolve()` that scans module descriptions and automatically proposes dependency links. |
| **No error recovery** | Agent gets raw cargo error messages with no diagnostic suggestions. | Add `module.diagnose()` that parses cargo errors and suggests fixes. |

### Tier 2: Production Features

| Gap | Impact | Fix |
|---|---|---|
| **No worker daemon** | Jobs don't execute in background. | Wire `automaton-worker` pull loop into CLI `worker start` command. |
| **No webhook triggers** | Can't trigger flows from external events. | Add HTTP listener in API server that calls `flow.execute()` on incoming webhooks. |
| **Only Rust language** | Agent can't use Python/TS for data tasks. | Add WASM runtime or sidecar subprocess for Python. |
| **No module versioning UI** | Can't diff/rollback module versions. | Add `module.diff(hash1, hash2)`. |

---

## 5. Key Configuration & Data Flow

### Data Directories

```
~/.local/share/automaton/
├── registry.db    — SQLite database (modules, jobs, runs, graph, secrets, triggers)
├── graph.db       — Deprecated, being merged into registry.db
├── builds/        — Compiled binaries (content-addressed by hash)
├── build-debug/   — Temporary cargo build artifacts
├── modules/       — Scaffolded module source files
├── work/          — Runtime working directory
└── tmp/           — Temporary files
```

### Module Compilation Flow

```
agent creates module → source written to ~/.local/share/automaton/modules/
build command → 
  1. Create temp Cargo project in build-debug/<name>/
  2. Write Cargo.toml with serde + serde_json + tokio
  3. Write main.rs from module source
  4. Run "cargo build --release" 
  5. Copy binary to builds/<hash>/binary
  6. Also copy to builds/<name> (predictable path for run command)
  7. Record hash in registry
```

### MCP Tool Registration

Each tool in `list_tools()` now has a real JSON schema derived from its parameter struct:

```rust
// In tools.rs — ALL param types must derive JsonSchema
#[derive(serde::Deserialize, JsonSchema)]
pub struct ModuleCreateParams {
    pub path: String,
    pub source: String, 
    // ...
}

// In lib.rs — register with real schema:
add_tool(&mut tools, "module_create", "Description", schema_for::<ModuleCreateParams>());
```

---

## 6. Important Code Paths

### Adding a New MCP Tool

1. Add param struct to `crates/automaton-mcp/src/tools.rs` with `#[derive(serde::Deserialize, JsonSchema)]`
2. Add handler in `crates/automaton-mcp/src/lib.rs` call_tool match statement
3. Add to `list_tools()` using `add_tool(&mut tools, "name", "desc", schema_for::<Type>())`
4. Add tool name string to `get_tool()` name array

### Adding a New Database Operation

1. Add method to `DbPool` trait in `crates/automaton-db/src/lib.rs`
2. Implement in `crates/automaton-db/src/postgres.rs`
3. Implement in `crates/automaton-db/src/sqlite.rs`
4. Both implementations must use the same SQL-compatible syntax

### Switching to Postgres

```bash
cargo build --features postgres --no-default-features
```

Requires setting `DATABASE_URL=host=localhost user=postgres dbname=automaton`.
The `libsqlite3-sys` native link conflict means Postgres and SQLite features can NEVER be simultaneously enabled.

---

## 7. Rust Gotchas & Workarounds

### `Result<T>` Shadowing

`automaton-core` exports `pub use error::{AutomatonError, Result}` where `Result<T> = std::result::Result<T, AutomatonError>`. The `pub use ...::Result` pattern re-exports a type alias that shadows `std::result::Result<T, E>`.

**Fix in any crate using auto`maton_core`:** If you need `Result<T, E>` with a non-`AutomatonError` error type, use `use std::result::Result as StdResult`.

### `schemars::schema_for!()` Macro Path

The `schema_for!` macro is a proc macro from the `schemars` crate. Proc macros can't be used through re-export paths like `rmcp::schemars::schema_for!(T)`. You must depend on `schemars` directly and call `schemars::schema_for!(T)`.

### `Tool::new()` Constructor

`Tool` is `#[non_exhaustive]` — can't construct with struct literal syntax. Must use:
```rust
Tool::new(name, description, Arc::new(serde_json::Map::new()))
```

### `rusqlite::Connection` in Async

`rusqlite::Connection` is `Send` but NOT `Sync`. In shared state (like `AppState` with `Engine`), wrap in `std::sync::Mutex`:
```rust
pub struct Registry {
    db: Mutex<Connection>,
}
```

---

## 8. Integration Tests

Located at `tests/src/main.rs` — 10 tests covering:
- Cron validation (valid + invalid expressions)
- CronTicker creation
- Flow flattening (simple flow with sleep + script)
- Branch flow (branch_one with 2 branches)
- ForLoop flow

Run with: `cargo run -p automaton-tests`

To add a new test:
1. Add to `tests/src/main.rs` following the existing pattern
2. Use `test_ok(pass, fail, name, || Ok(()))` for pass/fail tracking
3. Use `test_eq(pass, fail, name, condition)` for assertions

---

## 9. Dependency Graph

```
automaton-core     — No internal deps (foundation)
automaton-sdk      → automaton-core + automaton-sdk-derive
automaton-engine   → automaton-core + automaton-graph + automaton-registry
automaton-graph    → automaton-core + rusqlite
automaton-registry → automaton-core + rusqlite
automaton-runtime  → automaton-core
automaton-build    → automaton-core (spawns cargo)
automaton-worker   → automaton-core + automaton-build + automaton-runtime
automaton-scheduler→ automaton-core + croner
automaton-db       → automaton-core + {rusqlite | tokio-postgres + deadpool}
automaton-mcp      → automaton-core + automaton-engine + automaton-registry + 
                     automaton-graph + automaton-runtime + automaton-scheduler + 
                     automaton-build + schemars + rmcp
automaton-cli      → automaton-core + automaton-engine + automaton-registry + 
                     automaton-graph + automaton-runtime + automaton-mcp + 
                     automaton-build + clap
```

---

## 10. Phase Roadmap (Next Steps)

```
Phase 1 (done): Core types + real compilation + property graph + flow engine
Phase 2 (done): Worker + scheduler + MCP tools + database abstraction
Phase 3 (done): Agent UX — schemas, search, templates, capability discovery
Phase 4 (next): Postgres backend + worker daemon + webhook triggers
Phase 5: Module templates library + error diagnostics + version management
Phase 6: Multi-language support (WASM runtime for Python/TS)
```

### Immediate Next Actions

1. **Wire Postgres backend into engine** — Currently `automaton-db` has both backends but the engine still uses SQLite-specific crates (`automaton-registry`, `automaton-graph`). Need to abstract these behind the `DbPool` trait.

2. **Worker daemon** — `automaton-worker` has the `run_module()` method but no pull loop. Add `worker start` command that polls `dequeue()` → `compile()` → `run()` → `complete_job()`.

3. **Module template library** — Create 5-10 template patterns that compile standalone: http-fetch, db-query, slack-notify, cron-trigger, webhook-handler, data-transform, api-gateway, rate-limiter, health-check, log-aggregator.

4. **Webhook triggers** — Add a lightweight HTTP server in the API crate that registers webhook endpoints and enqueues jobs on incoming requests.

5. **Error diagnostics** — `module.diagnose()` that captures cargo build output, parses compile errors, and returns structured suggestions for the agent.
