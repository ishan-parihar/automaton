# Agent UX Flow: Building a GitHub Issue Triage Pipeline

This document walks through the complete experience of an **AI agent** using Automaton to build, compose, and manage a real-world automation pipeline — all through MCP tools.

---

## Scenario

An AI agent needs to build an **automated GitHub issue triage pipeline**:

1. Fetch new issues from a GitHub repository
2. Classify each issue by priority using LLM analysis
3. Send a Slack alert for high-priority issues
4. Store all results in a database for reporting

The agent has never done this before. It discovers Automaton's capabilities through MCP tools, creates each module, composes them into a flow, schedules it, and monitors its execution — all autonomously.

---

## Phase 1: Discovery

The agent connects to Automaton's MCP server and introspects available tools.

```
Agent → MCP tools.list()
Server → [module.create, module.build, module.run, flow.create,
          flow.execute, schedule.create, secret.set, resource.bind,
          graph.query, graph.add_edge, run.logs, ...]
```

The agent sees **24 tools** organized by category. It reads the `system.health` tool to understand the current state:

```
Agent → system.health()
Server → { status: "healthy", registry_modules: 3, graph_nodes: 3, ... }
```

The infrastructure exists but is empty. The agent needs to build everything.

---

## Phase 2: Module Creation

The agent creates four modules, each as a standalone Rust binary:

### Module 1: `github.fetch_issues`

```
Agent → module.create({
  path: "github.fetch_issues",
  source: "
    fn main() {
      let issues = fetch_github_issues(args.repo);
      println!(\"{}\", serde_json::to_string(&issues));
    }
  ",
  summary: "Fetch open issues from a GitHub repo"
})

Server → { status: "created", path: "github.fetch_issues", hash: "a1b2..." }

Agent → module.build({ path: "github.fetch_issues" })
Server → { status: "built", binary: ".../builds/github_fetch_issues", hash: "a1b2..." }
```

### Module 2: `llm.classify_priority`

```
Agent → module.create({
  path: "llm.classify_priority",
  source: "...",
  depends_on: ["github.fetch_issues"],
  summary: "Classify issue priority using LLM"
})
```

### Module 3: `slack.send_alert`

```
Agent → module.create({
  path: "slack.send_alert",
  source: "...",
  depends_on: ["llm.classify_priority"],
  summary: "Send Slack alert for critical issues"
})
```

### Module 4: `db.store_results`

```
Agent → module.create({
  path: "db.store_results",
  source: "...",
  depends_on: ["llm.classify_priority"],
  summary: "Store classification results in database"
})
```

Each module is compiled with `cargo build --release`, producing a **472KB native binary**. The build is content-addressed — rebuilding the same source returns the cached binary instantly.

---

## Phase 3: Configuration (Secrets & Resources)

The agent stores credentials needed by the modules:

```
Agent → secret.set({
  path: "github/token",
  value: "ghp_xxxx",
  description: "GitHub API token"
})
Server → { status: "secret_stored", path: "github/token" }

Agent → secret.set({
  path: "slack/webhook_url",
  value: "https://hooks.slack.com/xxx",
  description: "Slack alert webhook"
})

Agent → resource.bind({
  path: "github/production",
  resource_type: "github",
  value: { token: "$var:github/token", owner: "myorg", repo: "myrepo" }
})

Agent → resource.bind({
  path: "slack/engineering",
  resource_type: "slack",
  value: { webhook: "$var:slack/webhook_url", channel: "#alerts" }
})
```

At runtime, `$var:` references are automatically resolved to decrypted values. The agent never handles raw credentials.

---

## Phase 4: Flow Composition

Now the agent composes the four modules into a **DAG pipeline** using the flow engine. It defines steps with dependencies, branches, and error handling:

```
Agent → flow.create({
  path: "github.issue_triage_pipeline",
  steps: [
    {
      id: "fetch",
      kind: "Script",
      script_path: "github.fetch_issues",
      input: { repo: "$res:github/production", state: "open" },
      timeout_ms: 30000,
      retry: { max_attempts: 3, backoff: "Exponential", delay_ms: 1000 }
    },
    {
      id: "classify",
      kind: "Script",
      script_path: "llm.classify_priority",
      input: { issues: "{{fetch.output}}" },
      depends_on: ["fetch"],
      timeout_ms: 60000
    },
    {
      id: "notify",
      kind: "branch_one",
      depends_on: ["classify"],
      branches: [
        [{ id: "slack_alert", kind: "Script", script_path: "slack.send_alert",
           input: { issue: "{{classify.output.critical}}" }, depends_on: ["classify"] }],
        [{ id: "log_only", kind: "Sleep", sleep_after_ms: 100, depends_on: ["classify"] }]
      ]
    },
    {
      id: "store",
      kind: "Script",
      script_path: "db.store_results",
      input: { results: "{{classify.output}}" },
      depends_on: ["classify"],
      failure_step: "store_fallback"
    },
    {
      id: "store_fallback",
      kind: "FailureModule",
      script_path: "slack.send_alert",
      input: { error: "Database unavailable", severity: "critical" },
      depends_on: ["store"]
    }
  ],
  on_failure: "notify_admin",
  summary: "End-to-end GitHub issue triage pipeline"
})

Server → { status: "flow_created", path: "github.issue_triage_pipeline", steps: 5 }
```

The flow engine validates:

- **No cyclic dependencies** (toposort via petgraph)
- **All referenced modules exist** (registry lookup)
- **Branch configs are valid** (branch_one: first success / branch_all: all execute)
- **Failure handlers are defined** (store → store_fallback)

---

## Phase 5: Graph Wiring

The agent updates the **property graph** to reflect the new infrastructure. This enables visual exploration:

```
Agent → graph.add_edge({
  source: "github.fetch_issues",
  target: "llm.classify_priority",
  kind: "DEPENDS_ON"
})

Agent → graph.add_edge({
  source: "llm.classify_priority",
  target: "slack.send_alert",
  kind: "TRIGGERS"
})

Agent → graph.add_edge({
  source: "llm.classify_priority",
  target: "db.store_results",
  kind: "TRIGGERS"
})

Agent → graph.add_edge({
  source: "slack.alert",
  target: "github/token",
  kind: "USES_RESOURCE"
})
```

Now the agent can query the graph:

```
Agent → graph.query({ kind: "module" })
Server → { count: 4, nodes: [{ name: "github.fetch_issues", ... }, ...] }

Agent → graph.pathfind({ from: "github.fetch_issues", to: "slack.send_alert" })
Server → { paths_found: 1, paths: [[{ node: "fetch_issues" }, { node: "classify", edge: "DEPENDS_ON" }, { node: "send_alert", edge: "TRIGGERS" }]] }
```

---

## Phase 6: Scheduling

The agent schedules the pipeline to run hourly:

```
Agent → schedule.create({
  target_path: "github.issue_triage_pipeline",
  schedule: "0 * * * *",
  args: { repo: "myorg/myrepo" }
})

Server → { status: "schedule_created", target: "github.issue_triage_pipeline",
           schedule: "0 * * * *", valid_cron: true }
```

The scheduler validates the cron expression, registers the trigger, and starts ticking. Every hour at minute 0, the scheduler fires and queues a job.

---

## Phase 7: Execution & Monitoring

Each run creates a job in the queue, gets picked up by a worker, executes the pipeline, and stores results:

```
Agent → job.queue({
  target_path: "github.issue_triage_pipeline",
  kind: "flow",
  args: { repo: "myorg/myrepo" }
})

Server → { status: "queued", target: "github.issue_triage_pipeline", kind: "flow" }

Agent → run.logs({ module: "github.fetch_issues" })
Server → {
  count: 5,
  runs: [
    { state: "completed", duration_ms: 2450, created_at: "2026-05-06T..." },
    { state: "completed", duration_ms: 3100, created_at: "2026-05-06T..." },
    { state: "failed", error: "API rate limit", attempt: 3, created_at: "..." }
  ]
}
```

The agent sees execution history, retries failed runs, and optimizes:

```
Agent → run.retry({ run_id: "abc-123" })
Server → { status: "retry_scheduled", run_id: "abc-123" }
```

---

## Phase 8: Iteration

The agent iterates on the infrastructure. It inspects the flow, identifies bottlenecks, and refines:

```
Agent → workflow.plan({ start: "github.fetch_issues", max_depth: 10 })
Server → {
  run_graph_id: "flow-xxx",
  workflow: "github.issue_triage_pipeline",
  modules: [
    { module: "github.fetch_issues", depends_on: [], timeout_ms: 30000 },
    { module: "llm.classify_priority", depends_on: ["github.fetch_issues"], timeout_ms: 60000 },
    { module: "slack.send_alert", depends_on: ["llm.classify_priority"], ... },
    { module: "db.store_results", depends_on: ["llm.classify_priority"], ... }
  ],
  total_modules: 4
}
```

The agent adds a new module mid-pipeline:

```
Agent → module.create({
  path: "github.add_label",
  depends_on: ["llm.classify_priority"],
  ...
})

Agent → module.build({ path: "github.add_label" })

Agent → graph.add_edge({
  source: "llm.classify_priority",
  target: "github.add_label",
  kind: "DEPENDS_ON"
})

Agent → graph.query({ kind: "module" })
Server → { count: 5, nodes: [...] }
```

The infrastructure grows organically. Each new module integrates with existing ones through the property graph. The flow engine automatically detects dependencies, handles ordering, and validates the DAG.

---

## Complete UX Flow Diagram

```
┌────────────────────────────────────────────────────────────────────────┐
│                    AI AGENT WORKFLOW                                    │
├────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  DISCOVERY                                                              │
│  ┌────────────────┐                                                     │
│  │ tools.list()   │──► 24 MCP tools available                           │
│  │ system.health()│──► Infrastructure status                            │
│  └────────────────┘                                                     │
│                                                                         │
│  BUILD MODULES                                                          │
│  ┌────────────────┐                                                     │
│  │ module.create() │──► Register source + manifest                      │
│  │ module.build()  │──► cargo build --release → 472KB binary            │
│  │ secret.set()    │──► AES-encrypted credential storage                │
│  │ resource.bind() │──► Typed connection configs                        │
│  └────────────────┘                                                     │
│                                                                         │
│  COMPOSE FLOW                                                           │
│  ┌────────────────┐                                                     │
│  │ flow.create()  │──► DAG with branches, loops, error handlers         │
│  │ graph.add_edge()│──► Property graph wiring                           │
│  │ graph.pathfind()│──► Infrastructure visualization                    │
│  └────────────────┘                                                     │
│                                                                         │
│  SCHEDULE & EXECUTE                                                     │
│  ┌────────────────┐                                                     │
│  │ schedule.create()│──► Cron trigger registration                      │
│  │ job.queue()    │──► Enqueue for worker pool                          │
│  │ run.logs()     │──► Execution history inspection                     │
│  │ run.retry()    │──► Failed run recovery                              │
│  └────────────────┘                                                     │
│                                                                         │
│  ITERATE                                                                │
│  ┌────────────────┐                                                     │
│  │ workflow.plan()│──► Dependency discovery                             │
│  │ module.create()│──► Add new capabilities mid-pipeline                │
│  │ flow.execute() │──► Re-validate DAG                                  │
│  └────────────────┘                                                     │
│                                                                         │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Key Infrastructure Properties

| Property            | How Automaton Handles It                                                                                 |
| ------------------- | -------------------------------------------------------------------------------------------------------- |
| **Scalability**     | Each module = independent binary. Workers pull from queue via `FOR UPDATE SKIP LOCKED`.                  |
| **Reproducibility** | Content-addressed builds by SHA-256 hash. Same source → same binary.                                     |
| **Security**        | Secrets encrypted at rest with AES-256-GCM. `$var:` / `$res:` runtime injection.                         |
| **Observability**   | Every run persisted with full I/O + timing. Graph shows infrastructure state.                            |
| **Extensibility**   | New modules wire into existing graph via edges. Flow engine validates DAG.                               |
| **Resilience**      | Retry with exponential backoff. Failure modules handle errors. Circuit breakers stop cascading failures. |
| **Agent-Native**    | Everything available through MCP tools. No UI needed. Full autonomy.                                     |
