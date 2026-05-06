# Automaton → Windmill Production Audit

## Overview

Comparing Automaton (current Rust implementation) against Windmill's production architecture to identify every gap, prioritized by criticality.

---

## 1. Database Architecture

### Windmill
```
PostgreSQL 15+
├── workspace           # Multi-tenant isolation
├── script              # Hash-keyed, immutable, parent-chain versioning
├── queue               # queued_jobs table with FOR UPDATE SKIP LOCKED
├── completed_jobs      # Full result persistence
├── resource            # Typed, path-based (u/g/f prefixes)
├── variable            # Encrypted secrets, per-workspace key
├── trigger             # Cron, webhook, event schedules
├── flow_status         # Running flow state (JSONB)
└── oauth               # Provider credentials + refresh tokens
```

### Automaton (Current)
```
SQLite 3.x
├── modules              # Path-keyed, no versioning
├── dependencies         # Flat module-to-module
├── builds               # Build records (no content cache)
├── runs                 # Run history
└── resources            # Unencrypted key-value

graph.db (separate SQLite)
├── nodes                # UUID-based
├── edges                # UUID-based
```

### Gap
| Criterion | Windmill | Automaton | Fix Priority |
|---|---|---|---|
| Distributed | Yes (Postgres WAL + SKIP LOCKED) | No (single-file SQLite) | **P0** |
| Queue | `queued_jobs` with priority, concurrency tags | None | **P0** |
| Immutable versions | Hash-keyed, parent chain | Overwrite by path | **P0** |
| Encrypted secrets | AES-256-GCM, per-workspace key | Plaintext | **P1** |
| Flow state | JSONB persistence | None | **P1** |
| Transactions | ACID across queue + state | Partial | **P0** |

**Action: Replace SQLite with Postgres (sqlx + deadpool).** Single `automaton` database with schemas: `workspace`, `script`, `queue`, `flow`, `resource`, `variable`, `trigger`, `oauth`.

---

## 2. Compilation Pipeline

### Windmill
```
1. Auto-detect imports → generate lockfile
2. python-build → pip install --no-deps → venv tarball
3. deno-build → esbuild bundle → single .js
4. rust-build → cargo build --target wasm32-wasi → .wasm
5. Cache: source/{hash1} → module/{hash2} → result/{hash3}
   Triple-layer content-addressable cache
6. Build modes: debug (fast iteration), release (production)
```

### Automaton (Current)
```
"build" = no-op: marks module as built in SQLite, records path
No actual compilation happens
```

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Real compilation | WASM, venv, esbuild | No-op | **P0** |
| Content cache | 3-layer by hash | None | **P0** |
| Dependency resolution | Auto lockfile | Manual `depends_on` | **P1** |
| Build modes | debug/release | None | **P1** |
| Incremental | Only changed modules rebuild | N/A | **P2** |

**Action: Implement real `cargo build` for Rust modules.** Target WASM or native binary. Content-addressable build cache at `~/.automaton/builds/{hash}/`. Lockfile generation from module imports.

---

## 3. Worker Architecture

### Windmill
```
┌──────────┐    ┌──────────┐    ┌──────────┐
│ Worker 1 │    │ Worker 2 │    │ Worker N │
│ Rust CLI │    │ Rust CLI │    │ Rust CLI │
└────┬─────┘    └────┬─────┘    └────┬─────┘
     │ POLL          │ POLL          │ POLL
     │ SKIP LOCKED   │ SKIP LOCKED   │ SKIP LOCKED
     └───────────────┴───────────────┘
                    │
         ┌──────────▼──────────┐
         │   Postgres Queue    │
         │  queued_jobs table  │
         └─────────────────────┘

Each worker:
1. Polls job from queue (FOR UPDATE SKIP LOCKED)
2. Downloads source + dependencies
3. Runs in sandbox (wasmtime / subprocess)
4. Writes result + logs
5. Marks job complete
```

### Automaton (Current)
No worker system exists. The `run` command simulates execution.

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Pull-based queue | FOR UPDATE SKIP LOCKED | None | **P0** |
| Horizontal scaling | Add more workers | None | **P0** |
| Worker groups | Tag-based routing | None | **P2** |
| Heartbeat | TTL on in-progress jobs | None | **P1** |
| Concurrency | 2000+ per worker | None | **P2** |
| Graceful shutdown | Drain then stop | N/A | **P1** |

**Action: Create `automaton-worker` crate.** Pull loop → compile → execute → result. Configurable concurrency, worker tags, heartbeat TTL.

---

## 4. Flow / DAG Engine

### Windmill
```
openflow.yaml:
  steps:
    - id: fetch_data
      kind: script
      parent_step: null
    
    - id: process
      kind: script  
      parent_step: fetch_data
    
    - id: notify
      kind: script
      parent_step: process
      retry:
        max_duration: 300
        max_attempts: 5
    
    - id: fallback
      kind: script
      parent_step: fetch_data
      condition: notify failed
      failure_module: true

    - id: parallel
      kind: branch_one
      parent_step: process
      branches:
        - email_notify
        - slack_notify
        - log_only
    
    - id: loop_items
      kind: forloop
      parent_step: fetch_data
      iterator: results

    - id: sleep_step
      kind: sleep
      parent_step: loop_items
      sleep: 3600
```

Flow constructs: `branch_one` (first success), `branch_all` (all), `forloop` (iterable), `whileloop` (condition), `sleep` (timer), `failure_module` (error handler), `stop_after_if` (circuit breaker), `retry_if` (conditional retry), `retry_until` (poll until condition).

### Automaton (Current)
```rust
ModuleNode {
    depends_on: Vec<String>,  // Linear only
    retry: Option<RetryConfig>,  // Flat retry
    // No branches, no loops, no failure handlers, no sleep
}
```

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Linear deps | ✅ | ✅ | Done |
| branch_one | ✅ | ❌ | **P1** |
| branch_all | ✅ | ❌ | **P1** |
| forloop | ✅ | ❌ | **P2** |
| whileloop | ✅ | ❌ | **P2** |
| sleep step | ✅ | ❌ | **P1** |
| failure_module | ✅ | ❌ | **P1** |
| stop_after_if | ✅ | ❌ | **P2** |
| retry_if | ✅ | ❌ | **P1** |
| retry_until | ✅ | ❌ | **P2** |
| Step transforms | JS/JSONata/AI | ❌ | **P1** |
| Flow state | getState/setState | ❌ | **P1** |

**Action: Extend `ModuleNode` into a proper FlowNode enum.** Each variant has different execution semantics. Parser for OpenFlow YAML. Executor that handles all variants.

---

## 5. API Server

### Windmill
```
Axum REST API:
  POST   /api/w/{workspace}/scripts/create
  POST   /api/w/{workspace}/scripts/update
  GET    /api/w/{workspace}/scripts/get/{hash}
  POST   /api/w/{workspace}/scripts/run
  POST   /api/w/{workspace}/flows/create
  POST   /api/w/{workspace}/flows/run/{id}
  GET    /api/w/{workspace}/jobs/runs
  POST   /api/w/{workspace}/resources/create
  POST   /api/w/{workspace}/variables/create
  POST   /api/w/{workspace}/triggers/create
  GET    /api/w/{workspace}/oauth/{provider}
  ...
  
CLI (wmill) talks ONLY to API, never to DB directly.
```

### Automaton (Current)
CLI reads DB directly. No API layer.

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| REST API | Axum, tokio | None | **P0** |
| Auth | JWT, OAuth, API keys | None | **P0** |
| CLI→API separation | Yes | CLI reads DB | **P0** |
| Run queue submission | POST → insert queue | N/A | **P0** |
| Pagination | yes | None | **P1** |

**Action: Create `automaton-api` crate on Axum with shared types.** All operations go through REST. CLI becomes an HTTP client. MCP server becomes a consumer of the API.

---

## 6. Resource & Secret Management

### Windmill
```
Resources (typed connections):
  u/john/my_postgres:
    type: postgresql
    value: { host, port, db, user, password }
    description: "Production DB"

Variables (encrypted secrets):
  g/team/SLACK_TOKEN: xoxb-xxx-xxx-xxx
  f/marketing/ANTHROPIC_KEY: sk-ant-xxx

Resolution in scripts:
  $var:SLACK_TOKEN        → env variable (injected at runtime)
  $res:u/john/my_postgres → connection config object
  
Typesystem: Postgres, Slack, GitHub, Stripe, AWS, 30+ built-in types
```

### Automaton (Current)
```rust
pub struct Resource {
    pub path: String,
    pub resource_type: String,
    pub value: serde_json::Value,   // Plaintext
}
```

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Typed resources | 30+ built-in types | Unvalidated string | **P1** |
| Encryption | AES-256-GCM workspace key | Plaintext | **P1** |
| $var:/$res: syntax | Runtime injection | None | **P1** |
| Path hierarchy | u/g/f prefixes + owner | Flat | **P1** |
| OAuth credentials | Auto-refresh | None | **P2** |

**Action: Implement secret storage with AES-GCM encryption, $var:/$res: resolver in runtime, typed resource verification at deploy time.**

---

## 7. Triggers & Schedules

### Windmill
```
Triggers:
  ┌──────────┐  ┌──────────┐  ┌──────────┐
  │  Cron    │  │ Webhook  │  │  Event   │
  │  */5 * * │  │ GET/POST │  │ Kafka    │
  │  . . . . │  │ /hooks/* │  │ Postgres  │
  └──────────┘  └──────────┘  └──────────┘

Cron features:
- hexagon/croner expression parsing
- Multiple schedules per script
- skip: JavaScript expression to conditionally skip
- timezone-aware
- args: transforms arguments per schedule

Webhook features:
- Auto-generated endpoint per script
- HMAC signature verification
- Idempotency key support
- Custom URL path

Event triggers:
- Kafka topic consumer
- Postgres WAL (logical replication)
- SQS queue
- Email (IMAP)
```

### Automaton (Current)
No trigger system.

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Cron parsing | hexagon/croner | None | **P1** |
| Skip handlers | JS expression | None | **P2** |
| Multiple schedules | Per-script | N/A | **P2** |
| Webhook triggers | Auto endpoint | None | **P2** |
| Event triggers | Kafka/WAL/SQS | None | **P3** |
| Idempotency | Key-based dedup | None | **P2** |

**Action: Add `automaton-scheduler` crate with cron parser (hexagon port), schedule table, ticker loop. Webhook handler in API server.**

---

## 8. Multi-Tenancy & Organization

### Windmill
```
Instance
├── Workspace "prod"
│   ├── Folder "backend"
│   │   ├── Script publish_post
│   │   ├── Flow holiday_schedule
│   │   └── Resource pg_production
│   ├── Folder "frontend"
│   │   └── Script build_assets
│   ├── User alice (admin)
│   └── User bob (writer)
│
├── Workspace "staging"
│   ├── Folder "backend"
│   └── ...
│
└── OAuth
    ├── Google Workspace
    └── GitHub SSO

Permissions:
- Workspace: admin, operator, viewer
- Folder: writer, viewer
- Script: owner, editor, viewer
```

### Automaton (Current)
No organization. Single user, flat namespace.

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Workspaces | Isolated envs | None | **P2** |
| Folders | Hierarchical | None | **P2** |
| Users + auth | Email, SSO, API keys | None | **P2** |
| RBAC | Per-folder roles | None | **P3** |
| Audit log | All operations | `runs` table | **P2** |

**Action: Add workspace/folder schema to Postgres, JWT-based auth, role model. Phase 3 item.**

---

## 9. CLI & Deploy

### Windmill
```
wmill sync pull     # Download workspace → filesystem
wmill sync push     # Upload filesystem → workspace
wmill run <script>  # Run without deploying
wmill deploy        # Push + version
wmill script create # Interactive scaffolding
wmill app deploy    # Deploy UI apps

All JSON output via --json flag.
Hash-based deploy: every push creates a new immutable version.
```

### Automaton (Current)
```
automaton new       # Create scaffold files + register
automaton build     # Simulate build
automaton list      # List registered
automaton run       # Fail (not built)
automaton plan      # Show DAG
automaton mcp       # Start MCP server

No push/pull/deploy. No versioning.
```

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Sync pull/push | Bidirectional | None | **P2** |
| Hash versioning | Immutable, parent chain | Overwrite | **P0** |
| Deploy command | Promote version | None | **P2** |
| JSON output | --json everywhere | Already good | ✅ |
| MCP server | None | Already have | ✅ |

---

## 10. Error Handling & Recovery

### Windmill
```
Per-step error handling:
  retry:
    max_duration: 300s        # Instead of max_attempts
    max_attempts: 5           # With backoff
    jitter: 2s                # Random jitter
    multiplier: 2             # Exponential
    error_codes: [408, 429]   # Only retry these
    
  failure_module:              # Error handler module
    - log_error
    - send_alert
    - update_status
    
  stop_after_if:               # Circuit breaker
    expr: "result.count == 0"
    timeout: 1800s
    
  retry_if:                    # Conditional retry
    expr: "error.type == 'RateLimit'"
    backoff:
      multiplier: 3
      max_attempts: 10
      
  retry_until:                 # Poll until condition
    expr: "result.status == 'completed'"
    timeout: 3600s
```

### Automaton (Current)
```rust
RetryConfig {
    max_attempts: 3,       // Only count-based
    delay_ms: 1000,        // Flat or exponential
    backoff: BackoffKind,  // Fixed | Linear | Exponential
}
```

### Gap
| Feature | Windmill | Automaton | Priority |
|---|---|---|---|
| Duration-based retry | `max_duration` | Count only | **P1** |
| Jitter | Random delay | None | **P2** |
| Error code filter | Selective retry | All-or-nothing | **P1** |
| Failure module | Run on error | None | **P1** |
| Circuit breaker | Stop on condition | None | **P2** |
| Conditional retry | Only for specific errors | None | **P1** |

---

## Production Plan by Phase

### Phase 1 (P0 — immediate, 2 weeks)
```
1. Postgres migration (sqlx)
   ├── Replace SQLite with Postgres
   ├── Module + queue + run schema
   ├── Deadpool connection pool
   └── Migration system

2. Compilation pipeline
   ├── Real cargo build integration
   ├── Content-addressable build cache
   ├── Lockfile generation
   └── Debug/release mode distinction

3. API server (Axum)
   ├── REST endpoints for all operations
   ├── CLI becomes HTTP client
   ├── Run queue (INSERT → queued_jobs)
   └── Health + metrics

4. Worker crate
   ├── Pull loop (SELECT ... FOR UPDATE SKIP LOCKED)
   ├── Execute compiled binary
   ├── Write results + logs
   └── Concurrency control
```

### Phase 2 (P1 — 2–4 weeks)
```
5. Advanced DAG engine
   ├── branch_one, branch_all
   ├── Error handlers (failure_module)
   ├── Step transforms (JSONata)
   ├── Flow state (getState/setState)
   └── Circuit breaker (stop_after_if)

6. Secrets management
   ├── AES-256-GCM encryption
   ├── $var: / $res: resolver
   ├── Typed resources
   └── OAuth credential storage

7. Expanded retry
   ├── Duration-based (max_duration)
   ├── Jitter
   ├── Error code filtering
   └── Conditional retry (retry_if)

8. Scheduler
   ├── Cron parser (hexagon port)
   ├── Schedule table + ticker
   ├── Skip handlers
   └── Webhook triggers
```

### Phase 3 (P2 — 4–8 weeks)
```
9. Multi-language
   ├── Python (PyO3 / wasm)
   ├── TypeScript (deno_core)
   ├── Go (tinygo WASM)
   └── Common runtime interface

10. CLI sync
    ├── wmill-style sync pull/push
    ├── Hash-based versioning
    ├── Deployment promotion
    └── Directory watcher

11. Webhooks + events
    ├── Auto webhook per script
    ├── HMAC verification
    ├── Kafka consumer
    └── Idempotency keys

12. Multi-tenancy
    ├── Workspace isolation
    ├── Folder hierarchy
    ├── User management
    └── Basic RBAC
```

### Phase 4 (P3 — 8–12 weeks)
```
13. OAuth flows
    ├── 3-legged OAuth
    ├── Auto token refresh
    ├── Provider templates
    └── Workspace-level credentials

14. Enterprise
    ├── Full RBAC
    ├── Audit logs
    ├── Worker groups
    ├── Resource limits
    └── Hub marketplace
```

---

## Current Automaton Strengths (Keep)

```
✅ MCP server (rmcp-based) — AI-agent native control plane
✅ Property graph (design + run separation)
✅ Content-hash module identity
✅ Rust-first: 8.2 MB release binary (can strip to ~3MB)
✅ petgraph DAG with toposort cycle detection
✅ JSON output throughout CLI
✅ Workspace structure (9 crates, clean separation)
✅ SDK proc macro for #[automaton]
✅ `automaton init|new|build|run|list|show|graph|plan|mcp|doctor`
```

## Immediate Actions (this week)

1. `cargo add sqlx --features postgres,runtime-tokio-rustls` to `automaton-core`
2. Create `automaton-api` crate with Axum + shared route types
3. Implement real `cargo build` in `automaton-build` crate
4. Create `queued_jobs` Postgres table with `FOR UPDATE SKIP LOCKED`
5. Create `automaton-worker` crate with pull loop

---

## Appendix: Windmill DB Schema (Key Tables)

```sql
-- workspace
id UUID, name TEXT, visible BOOL, deleted BOOL

-- script
hash TEXT PRIMARY KEY, workspace_id UUID REFERENCES workspace,
path TEXT, parent_hash TEXT REFERENCES script(hash),
summary TEXT, description TEXT, schema JSONB,
kind INT, language TEXT, content TEXT, lockfile TEXT,
is_template BOOL, created_by TEXT, approved BOOL

-- queue
id BIGSERIAL PRIMARY KEY, workspace_id UUID,
job_kind TEXT (queued_job/flow_step/identity),
script_path TEXT, pre Runs ON SCHEDULE, running BOOL,
args JSONB, tag TEXT, priority INT, created_at TIMESTAMPTZ,
scheduled_for TIMESTAMPTZ

-- completed_job
id BIGINT PRIMARY KEY, workspace_id UUID,
script_path TEXT, args JSONB, result JSONB, logs JSONB,
permissioned_as TEXT, created_at TIMESTAMPTZ,
duration_ms BIGINT, raw_code TEXT, raw_lockfile TEXT

-- resource
path TEXT PRIMARY KEY, workspace_id UUID,
resource_type TEXT, value JSONB,
description TEXT IN

-- variable
path TEXT PRIMARY KEY, workspace_id UUID,
value TEXT (encrypted), description TEXT,
is_secret BOOL, account_id INT

-- trigger
workspace_id UUID, path TEXT, script_path TEXT,
is_flow BOOL, trigger_kind TEXT, args JSONB, enabled BOOL,

-- flow_status
workspace_id UUID, flow_id UUID, status JSONB,
type TEXT, modules JSONB
```
