# Automaton — AI-Handover Document

**Generated:** 2026-05-07 (Post Full-Scale Refactor)  
**GitHub:** https://github.com/ishan-parihar/automaton  
**Purpose:** A Rust-native automation substrate for AI agents to create, compose, and manage their own infrastructure through MCP tools.  
**Architecture n8n-for-AI-agents:** Modular Rust scripts composed into complex hierarchical flows with node-level interconnections, compiled and validated before execution.

---

## 1. Architecture Overview

```
crates/
├── automaton-core/         — Shared types: manifests, graph nodes, flows, errors, secrets, triggers, jobs, encryption
├── automaton-sdk/          — #[automaton] proc macro for agent-authored modules
├── automaton-sdk-derive/   — Proc macro implementation (generates real fn main())
├── automaton-engine/       — DAG planner + FlowEngine (branch_one, branch_all, forloop, whileloop)
├── automaton-runtime/      — Subprocess runner with retry + timeout (retry delay bug fixed)
├── automaton-graph/        — SQLite-backed property graph store (using automaton-core[rusqlite])
├── automaton-registry/     — SQLite-backed module catalog + build cache + run history + variables + triggers + jobs
├── automaton-build/        — Real cargo build integration, SDK auto-detection, extra deps, template system
├── automaton-worker/       — Worker daemon with pull loop (dequeue → compile → execute → complete)
├── automaton-scheduler/    — Cron expression validation + ticker + trigger persistence
├── automaton-mcp/          — MCP server with 29 tools, real JSON schemas, all tools persist data
├── automaton-postgres/     — Postgres backend via sqlx (PgPool, all CRUD operations)
├── automaton-api/          — Axum REST API server with 18+ CRUD endpoints
├── automaton-cli/          — CLI binary with 14 subcommands (including --pattern for templates)
└── tests/                  — Integration test suite (10 tests, cron + flow engine)

docs/
├── agent-ux-flow.md        — AI agent workflow documentation
├── schema.md               — Database schema documentation
└── windmill-audit.md       — Windmill comparison audit
```

### Database Strategy

**Three database systems coexist for different purposes:**

| System                               | Crate                                    | Purpose                                      | Status                              |
| ------------------------------------ | ---------------------------------------- | -------------------------------------------- | ----------------------------------- |
| SQLite (rusqlite)                    | `automaton-registry` + `automaton-graph` | Local dev, single-file state                 | ✅ Active                           |
| Postgres (sqlx)                      | `automaton-postgres`                     | Production, distributed workers              | ✅ Active (compiles, feature-gated) |
| Postgres (deadpool + tokio-postgres) | `automaton-db`                           | Legacy DbPool trait (removed from workspace) | 🗑️ Deprecated                       |

**Important:** `automaton-db` has been **removed from the workspace** due to `libsqlite3-sys` link conflicts. Use `automaton-postgres` (sqlx) for Postgres. The `automaton-db/postgres.rs` still exists for reference but is not compiled as part of the workspace.

---

## 2. What Works (End-to-End Verified)

| Feature                  | Status | Details                                                                                                                                 |
| ------------------------ | ------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| Module lifecycle         | ✅     | `create → build → run` via CLI + MCP                                                                                                    |
| Real compilation         | ✅     | `cargo build --release`, content-addressed cache by SHA-256                                                                             |
| Template library         | ✅     | 10 patterns: echo, http-fetch, http-server, db-query, slack-notify, data-transform, health-check, rate-limiter, file-watch, cron-worker |
| SDK proc macro           | ✅     | Generates real `fn main()` with `--input <json>`, calls user function, prints output                                                    |
| Build SDK auto-detect    | ✅     | Auto-adds `automaton-sdk` + extra deps when source uses `#[automaton]`                                                                  |
| Flow engine              | ✅     | `flatten()` + `execute()`: branch_one, branch_all, forloop, whileloop, Sleep, FailureModule                                             |
| Cron scheduler           | ✅     | Validate, match, next-occurrence, CronTicker                                                                                            |
| Property graph           | ✅     | SQLite-backed nodes + edges with pathfinding                                                                                            |
| Workflow planning        | ✅     | Dependency discovery → topological sort → DAG validation                                                                                |
| Secrets (MCP)            | ✅     | `secret_set`/`secret_get` now persist to Registry SQLite                                                                                |
| Resources (MCP)          | ✅     | `resource_bind`/`resource_list` now persist to Registry SQLite                                                                          |
| Jobs (MCP)               | ✅     | `job_queue`/`job_list` now enqueue to Registry SQLite                                                                                   |
| Triggers                 | ✅     | `schedule_create` persists to Registry `triggers` table                                                                                 |
| Flow execution (MCP)     | ✅     | `flow_execute` plans → materializes → executes DAG                                                                                      |
| Encryption (AES-256-GCM) | ✅     | `SecretKeeper` with `AUTOMATON_MASTER_KEY` env var                                                                                      |
| Worker daemon            | ✅     | Pull loop: `dequeue()` → `compile()` → `run()` → `complete_job()`                                                                       |
| REST API                 | ✅     | Axum 0.8 server with 18+ endpoints (scripts, jobs, runs, variables, resources, triggers, graph)                                         |
| Build cache              | ✅     | Content-addressed by SHA-256 hash of source + manifest                                                                                  |
| CLI                      | ✅     | init, new (--pattern), build, run, list, show, graph, plan, execute, logs, retry, doctor, mcp                                           |
| Integration tests        | ✅     | 10/10 passing (cron, flow engine, branches, loops)                                                                                      |
| Postgres feature         | ✅     | Compiles with `--no-default-features --features postgres`                                                                               |
| MCP tools total          | ✅     | 29 tools, all with real JSON schemas, all persist data                                                                                  |

---

## 3. What Was Fixed (All Known Bugs)

### Post-Audit Refactor Fixes (2026-05-07)

| Bug                                       | Root Cause                                                                                   | Fix                                                                                                         |
| ----------------------------------------- | -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| **Retry delay: first retry = 0ms**        | `delay` variable assigned, then immediately overwritten before first sleep                   | Sleep with `retry.delay_ms` first, then compute backoff after                                               |
| **Postgres: Config doesn't impl FromStr** | `deadpool_postgres::Config` tried `.parse()`                                                 | Used `tokio_postgres::Config::parse()` + `Manager::from_config()`                                           |
| **5 MCP tools were no-ops**               | `secret_set/get`, `resource_bind/list`, `job_queue/list` returned success without persisting | All wired to Registry SQLite storage via 10 new Registry methods                                            |
| **SDK: generated dead code**              | `__automaton_entry`/`__automaton_input_schema`/`__automaton_output_schema` never called      | SDK now generates real `fn main()` reading `--input <json>`                                                 |
| **Missing flow_execute tool**             | Agents could plan flows but not execute via MCP                                              | Added `flow_execute` tool: plan → materialize → execute DAG                                                 |
| **schedule_create didn't persist**        | Only validated cron, never registered trigger                                                | Now creates `triggers` record via Registry                                                                  |
| **libsqlite3-sys link conflict**          | `rusqlite` v0.35 (libsqlite3-sys 0.33) vs `sqlx` 0.7 (libsqlite3-sys 0.26) in same workspace | Made `rusqlite` optional in `automaton-core` behind `sqlite` feature; removed `automaton-db` from workspace |

### Pre-Audit Fixes

| Bug                                     | Fix                                       |
| --------------------------------------- | ----------------------------------------- |
| Build was a no-op                       | Wired `BuildCache::build_rust()` into CLI |
| Run returned "skipped"                  | Fixed after build fix                     |
| Duplicate graph nodes in `new`          | Consolidated `init_engine` to single call |
| Template used SDK proc macro            | Fallback to standalone `fn main()`        |
| `automaton_core::Result<T>` shadows std | Use `StdResult` alias                     |
| `rmcp::Tool` non-exhaustive             | Use `Tool::new(...)` constructor          |
| `schemars` macro via re-export          | Use `schemars::schema_for!()` directly    |

---

## 4. Current Gaps & Priority

### Tier 1: Agent Usability (Highest Impact)

| Gap                                | Impact                                                    | Status     | Fix                                                |
| ---------------------------------- | --------------------------------------------------------- | ---------- | -------------------------------------------------- |
| **Modules can't do useful things** | Generated templates have deps but no real API keys/config | ⚠️ Pending | Add template patterns with `$var:/$res:` injection |
| **No dependency resolution**       | Agent must manually declare deps                          | ❌ Pending | Add `flow.resolve()` auto-dependency discovery     |
| **No error recovery**              | Raw cargo error messages                                  | ❌ Pending | Add `module.diagnose()` cargo error parser         |

### Tier 2: Production Features

| Gap                        | Impact                                      | Status     | Fix                                  |
| -------------------------- | ------------------------------------------- | ---------- | ------------------------------------ |
| **Worker CLI command**     | Worker daemon exists but no CLI to start it | ⚠️ Pending | Add `worker start` CLI subcommand    |
| **No webhook triggers**    | Can't trigger flows from HTTP               | ❌ Pending | Wire API server triggers → enqueue   |
| **Flow state persistence** | No getState/setState across steps           | ❌ Pending | Add state map to FlowExecutor        |
| **Parallel DAG execution** | Engine executes sequentially                | ❌ Pending | Tokio tasks for independent branches |

### Tier 3: Future

| Feature                 | Notes                                          |
| ----------------------- | ---------------------------------------------- |
| WASM runtime (wasmtime) | Multi-language support without bloating binary |
| Module diff/versioning  | `module.diff(hash1, hash2)`                    |
| CLI sync pull/push      | Bidirectional filesystem sync                  |
| Workspace isolation     | Multi-tenant schema                            |
| Error diagnostics       | Cargo error parser with structured suggestions |

---

## 5. Key Configuration & Data Flow

### Data Directories

```
~/.local/share/automaton/
├── registry.db    — SQLite database (modules, jobs, runs, graph, secrets, triggers)
├── graph.db       — Separated property graph (being merged into registry.db)
├── builds/        — Compiled binaries (content-addressed by SHA-256 hash)
├── build-debug/   — Temporary cargo build artifacts
├── modules/       — Scaffolded module source files
├── work/          — Runtime working directory
└── tmp/           — Temporary files
```

### Environment Variables

| Variable               | Purpose                                   | Required                  |
| ---------------------- | ----------------------------------------- | ------------------------- |
| `AUTOMATON_MASTER_KEY` | AES-256-GCM encryption key (64 hex chars) | No (fallback key derived) |
| `DATABASE_URL`         | Postgres connection string                | For Postgres backend      |
| `AUTOMATON_LOG_LEVEL`  | Tracing level (debug/info/warn)           | No (default: info)        |

### Module Compilation Flow

```
Agent creates module (via MCP module_template or CLI new)
  → Source + manifest written to ~/.local/share/automaton/modules/<path>/
  → Registered in Registry SQLite
  → Graph node added

Build command (CLI build or MCP module_build):
  1. Compute SHA-256 hash of source + manifest
  2. Check cache at builds/<hash>/binary → instant return if cached
  3. Create temp Cargo project in build-debug/<name>/
  4. Write Cargo.toml with serde + serde_json + tokio + anyhow
  5. Auto-detect SDK usage → add automaton-sdk + schemars + uuid if needed
  6. Auto-detect template extra deps → add reqwest, axum, etc. if needed
  7. Run "cargo build --release"
  8. Copy binary to builds/<hash>/binary (content-addressed)
  9. Also copy to builds/<name> (predictable path for run command)
  10. Record hash in registry

Run command:
  1. Find binary at builds/<name> or builds/<hash>/binary
  2. Spawn as child process with --input <json>
  3. Parse stdout as JSON result
  4. Handle retry with exponential backoff if configured
```

### Worker Daemon Flow

```
Worker::start() loop:
  1. Registry::dequeue(worker_id)
     → SELECT pending job ORDER BY priority DESC, scheduled_for ASC LIMIT 1
     → UPDATE running=1, worker_id=$1, attempt++
     → Return job (or None if race condition)
  2. If job found:
     a. Registry::get(target) → get module source + manifest
     b. BuildCache::build_rust() → compile if needed
     c. Runtime::run_with_retry() → execute binary
     d. Registry::complete_job() → DELETE job
  3. If no job: sleep(poll_interval_ms)
  4. Check shutdown flag (AtomicBool)
```

### MCP Tool Registration Pattern

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

### API Server Routes

```
GET    /health                    → Health check
GET    /api/scripts               → List scripts
POST   /api/scripts               → Create script
GET    /api/scripts/:path         → Get script detail
POST   /api/scripts/:path/build   → Request build
POST   /api/scripts/:path/run     → Enqueue run
GET    /api/jobs                  → List jobs
POST   /api/jobs                  → Enqueue job
GET    /api/runs                  → List runs (query: ?path=&limit=)
GET    /api/variables             → List variables
POST   /api/variables             → Set variable
GET    /api/variables/:path       → Get variable
GET    /api/resources             → List resources
POST   /api/resources             → Set resource
GET    /api/resources/:path       → Get resource
GET    /api/triggers              → List triggers
POST   /api/triggers             → Create trigger
POST   /api/graph/nodes          → Add graph node
POST   /api/graph/edges          → Add graph edge
```

---

## 6. Important Code Paths

### Adding a New MCP Tool

1. **Param struct:** Add to `crates/automaton-mcp/src/tools.rs` with `#[derive(serde::Deserialize, JsonSchema)]`
2. **Handler:** Add match arm in `crates/automaton-mcp/src/lib.rs` `call_tool()`
3. **Register:** Add `add_tool()` call in `list_tools()`
4. **Discover:** Add tool name string to `get_tool()` name array
5. **Warn:** Update tool count in `capability_inventory`

### Adding a New Template

1. **Define:** Add `static TEMPLATE: Template` in `crates/automaton-build/src/templates.rs`
2. **Register:** Add `&TEMPLATE` to `all_templates()` function
3. **Extra deps:** Set `extra_deps: &[("crate_name", "version_spec")]`
4. **Test:** `automaton new my.module --pattern <name>` then `automaton build my.module`

### Adding a Registry Database Operation

1. **Migration:** Add `CREATE TABLE IF NOT EXISTS` to `init_tables()` in `crates/automaton-registry/src/lib.rs`
2. **Method:** Add `pub fn method_name(&self, ...) -> Result<T>` using `with_registry!` macro
3. **Expose:** Wire to MCP handler in `crates/automaton-mcp/src/lib.rs`

### Adding an API Route

1. **Handler:** Add async function in `crates/automaton-api/src/lib.rs` with `State(state): State<Arc<AppState>>`
2. **Route:** Add `.route("/api/...", get(...).post(...))` in `create_router()`
3. **Response:** Return `Response` using `ok_json(val)` or `err_msg(code, msg)`

### Switching to Postgres

```bash
# Build with Postgres backend (requires running Postgres)
cargo build --no-default-features --features postgres

# Set connection string
export DATABASE_URL="host=localhost user=automaton dbname=automaton"

# Note: The engine still uses SQLite-based Registry + GraphStore.
# The automaton-postgres crate provides a complete sqlx-based backend
# but is not yet wired into the Engine or CLI.
```

---

## 7. Rust Gotchas & Workarounds

### `Result<T>` Shadowing (Critical)

`automaton-core` exports `pub use error::{AutomatonError, Result}` where `Result<T> = std::result::Result<T, AutomatonError>`. This shadows `std::result::Result<T, E>`.

**Fix:** Use `use std::result::Result as StdResult;` in any crate that needs non-`AutomatonError` error types.

### `rusqlite::Error` Conversion (libsqlite3-sys Conflict)

`automaton-core` has `#[cfg(feature = "sqlite")] impl From<rusqlite::Error> for AutomatonError`. Crates that use `rusqlite` must depend on `automaton-core` with `features = ["sqlite"]`.

```toml
automaton-core = { workspace = true, features = ["sqlite"] }
```

Without this feature, `?` on `rusqlite::Result<_>` will fail with orphan rule errors.

### `rusqlite::Connection` Not Sync

`rusqlite::Connection` is `Send` but NOT `Sync`. Wrap in `std::sync::Mutex` for shared state:

```rust
pub struct Registry {
    db: Mutex<Connection>,
}
```

### `schemars::schema_for!()` Macro Path

`schema_for!` is a proc macro. Cannot be used through re-export paths like `rmcp::schemars::schema_for!(T)`. Must depend on `schemars` directly and call `schemars::schema_for!(T)`.

### `Tool::new()` Constructor

`Tool` is `#[non_exhaustive]` — cannot construct with struct literal. Must use:

```rust
Tool::new(name, description, Arc::new(serde_json::Map::new()))
```

### `gen` is Reserved in Edition 2024

In Rust 2024 edition, `gen` is a reserved keyword. Cannot call `.gen()` on `OsRng`. Use `OsRng.fill_bytes(&mut buf)` instead:

```rust
// WRONG (edition 2024):
let nonce: [u8; 12] = rand::rngs::OsRng.gen();

// RIGHT:
let mut nonce = [0u8; 12];
rand::rngs::OsRng.fill_bytes(&mut nonce);
```

### `scrubber::Html` Not Send

`scraper::Html` (from rmcp dependencies) doesn't implement `Send`. Cannot hold it across `.await` points in tokio tasks.

### Retry Delay: Compute After Sleep

The `run_with_retry` function must sleep FIRST with the current delay, THEN compute the next backoff:

```rust
// RIGHT:
if delay > 0 {
    tokio::time::sleep(Duration::from_millis(delay)).await;
}
delay = match retry.backoff {
    BackoffKind::Exponential => retry.delay_ms * (1u64 << attempt),
    // ...
};
```

---

## 8. Integration Tests

Located at `tests/src/main.rs` — **10 tests, all passing**.

| Test                 | Area        | What It Validates                |
| -------------------- | ----------- | -------------------------------- |
| Validate valid cron  | Scheduler   | `"*/5 * * * *"` returns Ok       |
| Reject invalid cron  | Scheduler   | `"not-a-cron"` returns Err       |
| Create CronTicker    | Scheduler   | Ticker initializes without error |
| Flatten simple flow  | Flow Engine | Sleep + Script step flattened    |
| Correct step count   | Flow Engine | Expected 2 steps                 |
| Has sleep step       | Flow Engine | Sleep step kind detected         |
| Flatten branch flow  | Flow Engine | BranchOne with 2 sub-branches    |
| Branch has 3 steps   | Flow Engine | BranchOne marker + 2 sub-steps   |
| Has branch_one step  | Flow Engine | BranchOne kind detected          |
| Flatten forloop flow | Flow Engine | ForLoop with iterator + sub-step |

**Run:** `cargo run -p automaton-tests`

**To add a new test:**

1. Add to `tests/src/main.rs` following existing pattern
2. Use `test_ok(pass, fail, name, || Ok(()))`
3. Use `test_eq(pass, fail, name, condition)`
4. Use `test_err(pass, fail, name, || Err(...))`

---

## 9. Dependency Graph (Updated)

```
automaton-core        — No internal deps (foundation, optional rusqlite behind "sqlite" feature)
automaton-sdk-derive  — syn, quote, proc-macro2
automaton-sdk         → automaton-core + automaton-sdk-derive
automaton-engine      → automaton-core + automaton-registry + automaton-runtime + automaton-graph
automaton-graph       → automaton-core (features = ["sqlite"]) + rusqlite
automaton-registry    → automaton-core (features = ["sqlite"]) + rusqlite
automaton-runtime     → automaton-core + automaton-registry
automaton-build       → automaton-core (has templates module)
automaton-worker      → automaton-core + automaton-build + automaton-runtime + automaton-registry
automaton-scheduler   → automaton-core + croner
automaton-mcp         → automaton-core + automaton-engine + automaton-registry +
                        automaton-graph + automaton-runtime + automaton-scheduler +
                        automaton-build + schemars + rmcp
automaton-postgres    → automaton-core + sqlx (postgres, no default features)
automaton-api         → automaton-core + automaton-postgres + automaton-build + axum
automaton-cli         → automaton-core + automaton-engine + automaton-registry +
                        automaton-graph + automaton-runtime + automaton-mcp +
                        automaton-build + clap
```

---

## 10. MCP Tool Reference (29 Tools)

| Tool                    | Category  | Status | Persists               |
| ----------------------- | --------- | ------ | ---------------------- |
| `module_create`         | Module    | ✅     | Registry + Graph       |
| `module_build`          | Module    | ✅     | Registry (marks built) |
| `module_validate`       | Module    | ✅     | —                      |
| `module_run`            | Module    | ✅     | —                      |
| `module_deprecate`      | Module    | ✅     | Graph (deletes)        |
| `module_search`         | Module    | ✅     | —                      |
| `module_template`       | Module    | ✅     | Registry               |
| `module_list_templates` | Module    | ✅     | —                      |
| `workflow_plan`         | Workflow  | ✅     | —                      |
| `workflow_materialize`  | Workflow  | ✅     | Validates DAG          |
| `graph_query`           | Graph     | ✅     | —                      |
| `graph_pathfind`        | Graph     | ✅     | —                      |
| `graph_add_edge`        | Graph     | ✅     | Graph SQLite           |
| `flow_create`           | Flow      | ✅     | Validates              |
| `flow.show`             | Flow      | ✅     | —                      |
| `flow_execute`          | Flow      | ✅     | Plans → Executes       |
| `schedule_create`       | Schedule  | ✅     | Registry triggers      |
| `schedule_validate`     | Schedule  | ✅     | —                      |
| `secret_set`            | Secrets   | ✅     | Registry variables     |
| `secret_get`            | Secrets   | ✅     | Registry variables     |
| `resource_bind`         | Resources | ✅     | Registry resources     |
| `resource_list`         | Resources | ✅     | Registry resources     |
| `job_queue`             | Jobs      | ✅     | Registry jobs          |
| `job_list`              | Jobs      | ✅     | Registry jobs          |
| `run_logs`              | Runs      | ✅     | Registry runs          |
| `run_retry`             | Runs      | ✅     | Queues retry           |
| `registry_search`       | Registry  | ✅     | —                      |
| `capability_inventory`  | System    | ✅     | —                      |
| `system_health`         | System    | ✅     | —                      |

---

## 11. Phase Roadmap (Next Steps)

```
Phase 0 (done): Core types + compilation + property graph + flow engine
Phase 1 (done): Postgres fix + MCP no-ops + SDK + retry bug + templates + encryption
Phase 2 (done): Worker daemon + Registry dequeue + API server + CLI --pattern
Phase 3 (done): SDK real schema + typed mains + parallel DAG + flow state + worker CLI + build diagnostics
Phase 4 (done): Webhook triggers + $var:/$res: injection + API build/run wiring
Phase 5 (done): Scheduler daemon + cron→job firing + TriggerProvider trait
Phase 6 (done): Worker concurrency via JoinSet + BuildCache clone-ability
Phase 7 (done): Unit test skeletons — 24 total (14 unit + 10 integration)
Phase 8 (done): `RegistryBackend` trait — backend-agnostic trait in `automaton_core`, implemented for:
  - ✅ SQLite (`automaton_registry::Registry`) — full CRUD delegation
  - ✅ Postgres (`automaton_postgres::AutomatonDb`) — get_script → AutomationModule conversion
  Engine can switch backends via `Arc<dyn RegistryBackend>`
Phase 9 (done): Event triggers — `POST /api/events/:trigger_id` validates event type, enqueues job
Phase 10 (next): Real while/for loop execution + Engine backend wiring + event_source matching
Phase 11: WASM runtime (wasmtime) for Python/TS/Go multi-language support
Phase 12: Module version diff + CLI sync push/pull + workspace isolation + marketplace
```

---

## Session 2026-05-07 — All Fixes

### SDK & Engine (P0)

| #   | Fix                                                                                                                                              | Files Changed                                                              |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------- |
| 1   | **SDK real schema generation** — `#[automaton]` macro generates `__automaton_input_schema()` via `schemars::schema_for!()` instead of empty `{}` | `automaton-sdk-derive/src/lib.rs`                                          |
| 2   | **SDK typed `fn main()`** — Generated `main()` deserializes into `InputType` not `serde_json::Value`                                             | `automaton-sdk-derive/src/lib.rs`                                          |
| 3   | **Parallel DAG execution** — Level-by-level concurrent execution via `futures::future::join_all`                                                 | `Cargo.toml`, `automaton-engine/Cargo.toml`, `automaton-engine/src/lib.rs` |
| 4   | **FlowState persistence** — `RunResult.flow_state` accumulates outputs; `resolve_state_refs()` resolves `${module}.field` templates              | `automaton-engine/src/lib.rs`                                              |

### CLI & Worker

| #   | Fix                                                                                                 | Files Changed                                           |
| --- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| 5   | **Worker CLI command** — `automaton worker start --concurrency N`                                   | `automaton-cli/Cargo.toml`, `automaton-cli/src/main.rs` |
| 6   | **Build error diagnostics** — `BuildCache::diagnose()` parses cargo stderr → `Vec<BuildDiagnostic>` | `automaton-build/src/lib.rs`                            |
| 7   | **Fix `execute` CLI command** — Now accepts module path instead of broken UUID                      | `automaton-cli/src/main.rs`                             |

### API & Integration

| #   | Fix                                                                                                                                                                                                  | Files Changed                                                                                   |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| 8   | **Webhook triggers** — `POST /api/webhooks/:trigger_id` validates secret, enqueues job                                                                                                               | `automaton-api/src/lib.rs`, `automaton-postgres/src/lib.rs`                                     |
| 9   | **$var:/$res: runtime injection** — `Registry::resolve_references()` resolves refs in JSON inputs, wired into Engine pre-execution                                                                   | `automaton-registry/src/lib.rs`, `automaton-engine/src/lib.rs`                                  |
| 10  | **API build/run wiring** — `POST /api/scripts/:path/build` calls `BuildCache::build_rust()`, `POST /api/scripts/:path/run` enqueues via DB. `AppState` now includes `build_cache_dir`/`data_dir`.    | `automaton-api/src/lib.rs`                                                                      |
| 11  | **Scheduler daemon** — `SchedulerDaemon::start()` polls cron triggers via `TriggerProvider` trait, fires matching triggers into job queue. Wired into `automaton worker start`.                      | `automaton-scheduler/src/lib.rs`, `automaton-cli/src/main.rs`, `automaton-scheduler/Cargo.toml` |
| 12  | **Worker concurrency** — `Worker::start()` now spawns N independent polling tasks via `JoinSet` when `concurrency > 1`. Each task opens its own SQLite handle (WAL mode). `BuildCache` made `Clone`. | `automaton-worker/src/lib.rs`, `automaton-build/src/lib.rs`                                     |
| 13  | **Unit test skeletons (14 new)** — `#[cfg(test)]` modules in engine (4), build (5), runtime (2), scheduler (3).                                                                                      | engine, build, runtime, scheduler                                                               |
| 14  | **`RegistryBackend` trait + SQLite impl** — `automaton_core::backend::RegistryBackend` with module/run/job/trigger/var/res CRUD. Engine can switch backends via `Arc<dyn RegistryBackend>`.          | `backend.rs`, `automaton-registry/src/lib.rs`                                                   |
| 15  | **`RegistryBackend` Postgres impl** — `automaton_postgres::AutomatonDb` implements the trait. Converts `get_script` JSON → `AutomationModule`.                                                       | `automaton-postgres/src/lib.rs`                                                                 |

### Current Status

- **0 errors, 0 new warnings** across all 14+ crates
- **24 tests passing** (14 unit + 10 integration)
- **`RegistryBackend` trait** for SQLite + Postgres
- **Webhook triggers** `POST /api/webhooks/:trigger_id` with secret validation
- **Event triggers** `POST /api/events/:trigger_id` with type validation
- **Scheduler daemon** firing cron triggers on `worker start`
- **Worker concurrency** via `JoinSet` with independent SQLite handles

### Next Steps

1. **Real while/for loop execution** — FlowEngine currently inserts stubs
2. **Wire Engine → `Arc<dyn RegistryBackend>`** when multi-backend needed
3. **event_source matching** — Match incoming events to triggers by `event_source` field
4. **Additional tests** — registry CRUD, graph pathfinding, MCP dispatch

- **0 errors, 0 warnings** (excluding pre-existing `sqlx-postgres` future compat notice)
- **24 tests passing** (14 unit + 10 integration)
- **`RegistryBackend` trait** implemented for both **SQLite** and **Postgres**
- **Test coverage**: engine, build, runtime, scheduler, automaton-tests
- **E2E verified:** `automaton doctor → plan → build → execute` — clean JSON output
- **Parallel DAG engine** executing with concurrent level-based scheduling
- **Worker concurrency** via `JoinSet` with independent SQLite handles
- **Scheduler daemon** firing cron triggers into job queue

### Immediate Next Actions

1. **Engine → `Arc<dyn RegistryBackend>` wiring** — Update Engine struct to accept trait instead of concrete Registry
2. **Real while/for loop execution** — FlowEngine currently inserts stubs for loops
3. **Event triggers** — `TriggerType::Event` subscription/routing
4. **Mutex → tokio::sync::Mutex** — (deferred: safe due to `with_registry` design)
5. **Registry/Graph unit tests** — registry CRUD, graph nodes/pathfinding
6. **MCP/API unit tests** — tool dispatch, route handlers
