# Database Schema for Automaton

## Design Principles

1. **Content-addressed immutability** — Every script/flow version is identified by a SHA-256 hash of its content. Paths are resolved to the latest hash. No overwrites, only new versions.

2. **Property graph** — Nodes + labeled edges for infrastructure visualization. Graph is the "source of truth" for how modules interconnect. SQL CTEs for pathfinding, petgraph in memory for computation.

3. **Queue-based workers** — `FOR UPDATE SKIP LOCKED` for Postgres, polling for SQLite. Jobs carry full context so workers are stateless.

4. **Secrets encrypted at rest** — AES-256-GCM per-value encryption. Master key stored in environment variable, never in the database.

5. **Single schema, two backends** — SQLite for local dev, Postgres for production. Same queries, different connection strings. Use `rusqlite` for SQLite, `sqlx` or `tokio-postgres` for Postgres.

---

## 1. Workspaces & Organization

```sql
CREATE TABLE IF NOT EXISTS workspaces (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    settings    JSONB DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS folders (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    parent_id   TEXT REFERENCES folders(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(workspace_id, name, parent_id)
);
```

**Why:** Even for a single-agent setup, workspaces isolate environments (dev/staging/prod). Folders let the agent organize modules by domain (social/, infra/, data/).

---

## 2. Scripts (Content-Addressed, Immutable)

```sql
CREATE TABLE IF NOT EXISTS scripts (
    -- Identity
    hash        TEXT PRIMARY KEY,           -- SHA-256 of source + manifest
    path        TEXT NOT NULL,              -- e.g. "social.post_scheduler"
    version     TEXT NOT NULL DEFAULT '0.1.0',
    parent_hash TEXT REFERENCES scripts(hash),  -- Previous version chain
    
    -- Content
    source      TEXT NOT NULL,              -- The Rust/TS/Python source code
    manifest    JSONB NOT NULL DEFAULT '{}', -- AutomationManifest as JSON
    
    -- Status
    built       BOOLEAN NOT NULL DEFAULT false,
    language    TEXT NOT NULL DEFAULT 'rust',
    
    -- Metadata
    folder_id   TEXT REFERENCES folders(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Script dependencies
CREATE TABLE IF NOT EXISTS script_deps (
    script_hash TEXT NOT NULL REFERENCES scripts(hash) ON DELETE CASCADE,
    depends_on  TEXT NOT NULL,              -- Path of dependency
    version_req TEXT,
    PRIMARY KEY (script_hash, depends_on)
);

-- Index: resolve path → latest hash
CREATE INDEX idx_scripts_path_hash ON scripts(path, created_at DESC());
```

**Key design:** `hash` = content address, `path` = human-readable name. `parent_hash` creates a version chain. To get the "current" version: `SELECT * FROM scripts WHERE path = ? ORDER BY created_at DESC LIMIT 1`.

**For the AI agent:** The agent creates a module at path `social.post_scheduler`. Each edit creates a new immutable version. The agent can always reference "the latest" or pin to a specific hash.

---

## 3. Flows (DAG Compositions)

```sql
CREATE TABLE IF NOT EXISTS flows (
    hash            TEXT PRIMARY KEY,             -- SHA-256 of definition
    path            TEXT NOT NULL,                -- e.g. "social.daily_pipeline"
    version         TEXT NOT NULL DEFAULT '0.1.0',
    parent_hash     TEXT REFERENCES flows(hash),
    
    definition      JSONB NOT NULL,               -- FlowDefinition as JSON
    -- Steps, branches, forloops, error handlers are all in the JSON
    
    folder_id       TEXT REFERENCES folders(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_flows_path ON flows(path, created_at DESC());
```

**Why JSONB?** Flow definitions are inherently recursive (a step can contain branches, which contain steps, which contain forloops). JSONB captures this without complex normalized tables. The `FlowEngine::flatten()` deserializes and validates at runtime.

---

## 4. Property Graph (Infrastructure Visualization)

```sql
-- Nodes
CREATE TABLE IF NOT EXISTS graph_nodes (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    workspace_id TEXT NOT NULL DEFAULT 'default',
    kind        TEXT NOT NULL,               -- module | flow | trigger | resource | capability
    name        TEXT NOT NULL,
    properties  JSONB NOT NULL DEFAULT '{}', -- Flexible key-value metadata
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Directed, labeled edges
CREATE TABLE IF NOT EXISTS graph_edges (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    workspace_id TEXT NOT NULL DEFAULT 'default',
    source      TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    target      TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,               -- DEPENDS_ON | CALLS | TRIGGERS | USES_RESOURCE | ...
    properties  JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fast lookups
CREATE INDEX idx_nodes_kind ON graph_nodes(workspace_id, kind);
CREATE INDEX idx_edges_source ON graph_edges(workspace_id, source);
CREATE INDEX idx_edges_target ON graph_edges(workspace_id, target);
CREATE INDEX idx_edges_kind ON graph_edges(workspace_id, kind);
```

**Pathfinding query (Postgres CTE):**
```sql
WITH RECURSIVE walk AS (
    SELECT source, target, kind, 1 AS depth, ARRAY[source, target] AS path
    FROM graph_edges WHERE source = 'start_node_id'
    UNION ALL
    SELECT e.source, e.target, e.kind, w.depth + 1, w.path || e.target
    FROM graph_edges e, walk w
    WHERE e.source = w.target AND w.depth < 10 AND NOT e.target = ANY(w.path)
)
SELECT * FROM walk WHERE target = 'target_node_id';
```

**For the AI agent:** The agent can query "what depends on my slack module?", "find a path from webhook to notification", or "show me all triggers connected to resources."

---

## 5. Job Queue (Worker Distribution)

```sql
CREATE TABLE IF NOT EXISTS jobs (
    id              BIGSERIAL PRIMARY KEY,
    workspace_id    TEXT NOT NULL DEFAULT 'default',
    
    -- What to run
    kind            TEXT NOT NULL DEFAULT 'script',  -- script | flow | flow_step
    target_path     TEXT NOT NULL,        -- Script or flow path
    args            JSONB NOT NULL DEFAULT '{}',
    
    -- Scheduling
    scheduled_for   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    priority        INT NOT NULL DEFAULT 0,     -- Higher = runs first
    tag             TEXT,                       -- Worker group tag ("rust", "python", "gpu")
    
    -- Execution
    running         BOOLEAN NOT NULL DEFAULT false,
    worker_id       TEXT,
    max_attempts    INT NOT NULL DEFAULT 3,
    attempt         INT NOT NULL DEFAULT 0,
    
    -- Lifecycle
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Worker pull pattern: FOR UPDATE SKIP LOCKED
CREATE INDEX idx_jobs_pending ON jobs(scheduled_for, priority DESC)
    WHERE NOT running AND scheduled_for <= NOW();
```

**Worker dequeue (Postgres):**
```sql
UPDATE jobs SET running = true, worker_id = $1, attempt = attempt + 1
WHERE id = (
    SELECT id FROM jobs
    WHERE NOT running AND scheduled_for <= NOW()
    ORDER BY priority DESC, scheduled_for ASC
    LIMIT 1
    FOR UPDATE SKIP LOCKED
)
RETURNING id, kind, target_path, args;
```

**Worker dequeue (SQLite):**
```sql
UPDATE jobs SET running = true, worker_id = $1, attempt = attempt + 1
WHERE id = (
    SELECT id FROM jobs
    WHERE NOT running AND scheduled_for <= NOW()
    ORDER BY priority DESC, scheduled_for ASC
    LIMIT 1
)
RETURNING id, kind, target_path, args;
```

**For the AI agent:** The agent queues a job via `job.queue`, workers pick it up, execute, store results. The agent can inspect the queue, retry failed jobs, and control concurrency.

---

## 6. Runs (Execution History)

```sql
CREATE TABLE IF NOT EXISTS runs (
    id              TEXT PRIMARY KEY,            -- UUID
    workspace_id    TEXT NOT NULL DEFAULT 'default',
    
    job_id          BIGINT REFERENCES jobs(id),
    target_path     TEXT NOT NULL,
    kind            TEXT NOT NULL DEFAULT 'script',
    
    -- Input/Output
    args            JSONB NOT NULL DEFAULT '{}',
    result          JSONB,
    error           TEXT,
    
    -- State machine
    state           TEXT NOT NULL DEFAULT 'pending',
    -- pending → running → completed | failed | skipped
    
    attempt         INT NOT NULL DEFAULT 1,
    duration_ms     BIGINT NOT NULL DEFAULT 0,
    
    -- Timeline
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ
);

CREATE INDEX idx_runs_target ON runs(workspace_id, target_path, created_at DESC());
CREATE INDEX idx_runs_state ON runs(workspace_id, state);
CREATE INDEX idx_runs_created ON runs(workspace_id, created_at DESC());
```

**For the AI agent:** Full execution history. The agent can see what ran, for how long, what it output, and what failed. Useful for debugging and optimization.

---

## 7. Secrets (Encrypted Variables)

```sql
CREATE TABLE IF NOT EXISTS variables (
    path            TEXT PRIMARY KEY,        -- e.g. "slack/api_token"
    workspace_id    TEXT NOT NULL DEFAULT 'default',
    
    encrypted_value TEXT NOT NULL,           -- AES-256-GCM ciphertext (base64)
    is_secret       BOOLEAN NOT NULL DEFAULT true,
    description     TEXT,
    
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_variables_workspace ON variables(workspace_id);
```

### Encryption Flow

```
Store:
  1. Generate random 12-byte nonce
  2. Encrypt(value, key, nonce) → ciphertext
  3. Store base64(nonce || ciphertext)

Retrieve:
  1. Decode base64 → nonce (first 12 bytes) + ciphertext
  2. Decrypt(ciphertext, key, nonce) → plaintext
  3. Inject as $var:SLACK_TOKEN → actual token at runtime
```

### $var: / $res: Resolution

At runtime, the runner scans script args for `$var:NAME` and `$res:PATH` patterns and replaces them with actual values from the variables/resources tables. This is how the agent creates reusable, secure configurations.

```
Script input:  { "api_key": "$var:slack/api_token" }
Runtime:       { "api_key": "xoxb-1234-5678-..." }  // decrypted at execution
```

---

## 8. Resources (Typed External Connections)

```sql
CREATE TABLE IF NOT EXISTS resources (
    path            TEXT PRIMARY KEY,        -- e.g. "slack/production"
    workspace_id    TEXT NOT NULL DEFAULT 'default',
    
    resource_type   TEXT NOT NULL,           -- postgresql | slack | github | openai | http | aws
    value           JSONB NOT NULL DEFAULT '{}',  -- Connection config
    description     TEXT,
    
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Built-in resource type schemas
CREATE TABLE IF NOT EXISTS resource_types (
    name            TEXT PRIMARY KEY,
    schema          JSONB NOT NULL,          -- JSON Schema for the resource value
    description     TEXT
);

INSERT INTO resource_types (name, schema, description) VALUES
    ('postgresql', '{"type":"object","properties":{"host":{"type":"string"},"port":{"type":"integer"},"database":{"type":"string"},"username":{"type":"string"},"password":{"type":"string","secret":true}}}', 'PostgreSQL connection'),
    ('slack',      '{"type":"object","properties":{"token":{"type":"string","secret":true},"team":{"type":"string"}}}', 'Slack workspace connection'),
    ('github',     '{"type":"object","properties":{"token":{"type":"string","secret":"true"},"owner":{"type":"string"}}}', 'GitHub API connection'),
    ('openai',     '{"type":"object","properties":{"api_key":{"type":"string","secret":true}}}', 'OpenAI API connection'),
    ('anthropic',  '{"type":"object","properties":{"api_key":{"type":"string","secret":true}}}', 'Anthropic API connection'),
    ('http',       '{"type":"object","properties":{"base_url":{"type":"string"},"headers":{"type":"object"},"timeout_ms":{"type":"integer"}}}', 'Generic HTTP endpoint'),
    ('aws',        '{"type":"object","properties":{"access_key_id":{"type":"string"},"secret_access_key":{"type":"string","secret":true},"region":{"type":"string"}}}', 'AWS credentials');
```

**For the AI agent:** The agent binds a Slack resource → runtime injects the token → modules use it. The agent creates a Postgres resource → flow steps query the database. All without hardcoding credentials.

---

## 9. Triggers (Cron, Webhook, Events)

```sql
CREATE TABLE IF NOT EXISTS triggers (
    id              TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    workspace_id    TEXT NOT NULL DEFAULT 'default',
    
    target_path     TEXT NOT NULL,           -- What to trigger
    target_is_flow  BOOLEAN NOT NULL DEFAULT false,
    
    trigger_type    TEXT NOT NULL DEFAULT 'cron',   -- cron | webhook | event
    config          JSONB NOT NULL DEFAULT '{}',
    -- Cron:  { "schedule": "*/5 * * * *", "timezone": "UTC", "skip_if": null, "args": {} }
    -- Webhook: { "secret": "whsec_...", "method": "POST", "path": "/hooks/..." }
    -- Event:  { "source": "kafka:topic", "filters": {} }
    
    enabled         BOOLEAN NOT NULL DEFAULT true,
    last_fired_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_triggers_enabled ON triggers(workspace_id, enabled, trigger_type);
```

### Scheduler Loop

```
Every 60 seconds:
  SELECT * FROM triggers WHERE enabled AND trigger_type = 'cron'
  For each trigger:
    IF cron.matches(now, config.schedule):
      INSERT INTO jobs (kind, target_path, args) VALUES (...)
      UPDATE triggers SET last_fired_at = NOW()
```

---

## 10. OAuth Credentials (For Social Media Integration)

```sql
CREATE TABLE IF NOT EXISTS oauth_credentials (
    id              TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    workspace_id    TEXT NOT NULL DEFAULT 'default',
    
    provider        TEXT NOT NULL,           -- slack | github | linkedin | twitter | instagram
    account_name    TEXT,                    -- Human-readable label
    access_token    TEXT NOT NULL,           -- Encrypted
    refresh_token   TEXT,                    -- Encrypted (for refresh flows)
    expires_at      TIMESTAMPTZ,             -- Token expiration
    
    -- Provider-specific metadata
    provider_user_id TEXT,
    provider_team_id TEXT,
    scopes          TEXT,
    
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    UNIQUE(workspace_id, provider, account_name)
);
```

---

## 11. Build Cache (Content-Addressed)

```sql
CREATE TABLE IF NOT EXISTS build_cache (
    hash            TEXT PRIMARY KEY,        -- SHA-256 of source + deps
    script_path     TEXT NOT NULL,
    
    language        TEXT NOT NULL,
    binary_path     TEXT NOT NULL,           -- Path to compiled artifact
    binary_size     BIGINT,                  -- Bytes
    build_duration_ms BIGINT,                -- Build time
    
    build_mode      TEXT NOT NULL DEFAULT 'debug',  -- debug | release
    success         BOOLEAN NOT NULL DEFAULT true,
    error_log       TEXT,
    
    built_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_build_cache_script ON build_cache(script_path);
```

---

## Complete Migration Script (SQLite-Compatible)

```sql
-- Workspace
CREATE TABLE IF NOT EXISTS workspaces (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, description TEXT, settings TEXT DEFAULT '{}', created_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Scripts
CREATE TABLE IF NOT EXISTS scripts (hash TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', path TEXT NOT NULL, version TEXT NOT NULL DEFAULT '0.1.0', parent_hash TEXT REFERENCES scripts(hash), source TEXT NOT NULL, manifest TEXT NOT NULL DEFAULT '{}', built INTEGER NOT NULL DEFAULT 0, language TEXT NOT NULL DEFAULT 'rust', folder_id TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS script_deps (script_hash TEXT NOT NULL REFERENCES scripts(hash) ON DELETE CASCADE, depends_on TEXT NOT NULL, version_req TEXT, PRIMARY KEY (script_hash, depends_on));

-- Flows
CREATE TABLE IF NOT EXISTS flows (hash TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', path TEXT NOT NULL, version TEXT NOT NULL DEFAULT '0.1.0', parent_hash TEXT REFERENCES flows(hash), definition TEXT NOT NULL DEFAULT '{}', folder_id TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Property Graph
CREATE TABLE IF NOT EXISTS graph_nodes (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', kind TEXT NOT NULL, name TEXT NOT NULL, properties TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS graph_edges (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', source TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE, target TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE, kind TEXT NOT NULL, properties TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Jobs
CREATE TABLE IF NOT EXISTS jobs (id INTEGER PRIMARY KEY AUTOINCREMENT, workspace_id TEXT NOT NULL DEFAULT 'default', kind TEXT NOT NULL DEFAULT 'script', target_path TEXT NOT NULL, args TEXT NOT NULL DEFAULT '{}', scheduled_for TEXT NOT NULL DEFAULT (datetime('now')), priority INTEGER NOT NULL DEFAULT 0, tag TEXT, running INTEGER NOT NULL DEFAULT 0, worker_id TEXT, max_attempts INTEGER NOT NULL DEFAULT 3, attempt INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Runs
CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', job_id INTEGER, target_path TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'script', args TEXT NOT NULL DEFAULT '{}', result TEXT, error TEXT, state TEXT NOT NULL DEFAULT 'pending', attempt INTEGER NOT NULL DEFAULT 1, duration_ms INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')), started_at TEXT, completed_at TEXT);

-- Secrets
CREATE TABLE IF NOT EXISTS variables (path TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', encrypted_value TEXT NOT NULL, is_secret INTEGER NOT NULL DEFAULT 1, description TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Resources
CREATE TABLE IF NOT EXISTS resources (path TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', resource_type TEXT NOT NULL, value TEXT NOT NULL DEFAULT '{}', description TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Triggers
CREATE TABLE IF NOT EXISTS triggers (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', target_path TEXT NOT NULL, target_is_flow INTEGER NOT NULL DEFAULT 0, trigger_type TEXT NOT NULL DEFAULT 'cron', config TEXT NOT NULL DEFAULT '{}', enabled INTEGER NOT NULL DEFAULT 1, last_fired_at TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')));

-- OAuth
CREATE TABLE IF NOT EXISTS oauth_credentials (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT 'default', provider TEXT NOT NULL, account_name TEXT, access_token TEXT NOT NULL, refresh_token TEXT, expires_at TEXT, provider_user_id TEXT, provider_team_id TEXT, scopes TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Build cache
CREATE TABLE IF NOT EXISTS build_cache (hash TEXT PRIMARY KEY, script_path TEXT NOT NULL, language TEXT NOT NULL, binary_path TEXT NOT NULL, binary_size INTEGER, build_duration_ms INTEGER, build_mode TEXT NOT NULL DEFAULT 'debug', success INTEGER NOT NULL DEFAULT 1, error_log TEXT, built_at TEXT NOT NULL DEFAULT (datetime('now')));

-- Indexes
CREATE INDEX IF NOT EXISTS idx_scripts_path ON scripts(workspace_id, path, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_flows_path ON flows(workspace_id, path, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON graph_nodes(workspace_id, kind);
CREATE INDEX IF NOT EXISTS idx_edges_source ON graph_edges(workspace_id, source);
CREATE INDEX IF NOT EXISTS idx_edges_target ON graph_edges(workspace_id, target);
CREATE INDEX IF NOT EXISTS idx_runs_target ON runs(workspace_id, target_path, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_triggers_type ON triggers(workspace_id, enabled, trigger_type);
CREATE INDEX IF NOT EXISTS idx_jobs_scheduled ON jobs(scheduled_for) WHERE running = 0;
```

## Implementation Order

```
Week 1: scripts + flows + jobs + runs       (core execution loop)
Week 2: graph_nodes + graph_edges           (infrastructure visualization)
Week 3: variables + resources               (secrets + connections)  
Week 4: triggers + oauth_credentials         (scheduling + social integration)
Week 5: workspaces + build_cache             (organization + performance)
```
