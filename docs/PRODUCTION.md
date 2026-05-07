# Production Deployment Guide

This guide covers deploying Automaton in production environments. It assumes you have a working Automaton binary and understand the basic CLI operations from the README.

---

## Section 1: Backend Storage

Automaton supports two storage backends through the `RegistryBackend` trait. Choose the one that fits your deployment.

### SQLite

The default backend. SQLite is embedded in the process, requires no external services, and is file-based. It is a good choice for single-node, single-agent setups where simplicity matters.

- Zero configuration. No connection string, no daemon to manage.
- The database is a single file on disk. Back it up with standard file tools.
- Best for development, personal deployments, and agents that do not share state.
- Not suitable for multi-process or multi-node concurrent access. SQLite serializes writes, so concurrent agent operations will contend.

### Postgres

The production-grade backend. Postgres handles concurrent reads and writes from multiple processes and supports the full set of Automaton features without contention.

- Requires a running Postgres instance and a connection string.
- Enables multi-node deployments where multiple Automaton processes share the same registry.
- Durable under heavy concurrent load. Parallel workers, schedulers, and API servers can all access the same database.
- Use Postgres when you run multiple agents, need high availability, or deploy across multiple machines.

---

## Section 2: Postgres Migration

When using the Postgres backend, you must run database migrations before starting any Automaton processes. Migrations create the schema and are idempotent.

### CLI Command

```sh
automaton postgres migrate --database-url "postgres://user:password@host:5432/automaton"
```

If the `--database-url` flag is omitted, the CLI falls back to the `DATABASE_URL` environment variable.

```sh
export DATABASE_URL="postgres://user:password@host:5432/automaton"
automaton postgres migrate
```

### Schema

Migrations are managed with `sqlx`. The Postgres backend creates the following tables:

- `modules` -- registered automation modules
- `builds` -- build records for module compilation
- `runs` -- execution runs
- `resources` -- resource declarations
- `variables` -- scoped variables
- `triggers` -- cron and event triggers
- `flows` -- flow definitions
- `graph_nodes` -- nodes in a flow graph
- `graph_edges` -- edges connecting flow nodes
- `jobs` -- queued job records
- `webhooks` -- registered outgoing webhooks
- `executions` -- fine-grained execution traces

Run migrations once per database, typically as part of your provisioning process. Migrations are safe to run repeatedly; they skip already-applied changes.

---

## Section 3: Static Binary

Automaton compiles to a fully static binary with no runtime dependencies. Build it with musl targeting for maximum portability.

### Build Command

```sh
cargo build --release --target x86_64-unknown-linux-musl -p automaton-cli
```

### Artifact

The binary is produced at:

```
target/x86_64-unknown-linux-musl/release/automaton
```

It is roughly 14 MB in size and linked as a static-pie binary. Because it is statically linked against musl libc, it has **no GLIBC dependency** and runs on any Linux x86_64 distribution. It has been tested on Debian 12 (which ships GLIBC 2.36), but it also runs on older distributions and distroless base images.

### What Is Included

The single binary contains both the CLI interface and the MCP server logic. There are no shared libraries, sidecar processes, or runtime assets to deploy. Copy the binary to the target machine and run it.

---

## Section 4: Rate Limiting

The REST API uses semaphore-based rate limiting to prevent resource exhaustion under load.

### How It Works

A `tokio::sync::Semaphore` gates incoming requests. Each request acquires a permit before processing and releases it on completion. When all permits are exhausted, new requests are rejected immediately.

### Configuration

The maximum number of concurrent requests is controlled by the `MAX_CONCURRENT_REQUESTS` environment variable.

| Variable | Default | Description |
|---|---|---|
| `MAX_CONCURRENT_REQUESTS` | `100` | Maximum concurrent REST API requests |

### Limit Response

When the limit is hit, the API returns HTTP 429 with a JSON body:

```json
{
  "error": "rate_limit_exceeded",
  "message": "Too many concurrent requests"
}
```

Set `MAX_CONCURRENT_REQUESTS` according to your expected workload and available system resources. A higher value allows more concurrency but consumes more memory and file descriptors.

---

## Section 5: Webhooks

Automaton supports outbound webhooks that fire on automation lifecycle events. Webhooks are registered, listed, and deleted through MCP tools.

### Registration

Webhooks are managed via the `RegistryBackend` trait and exposed through three MCP tools:

- `webhook_register` -- create a new webhook subscription
- `webhook_list` -- list all registered webhooks
- `webhook_delete` -- remove a webhook subscription

### Registration Parameters

Each webhook registration includes:

- `url` -- the target endpoint that receives the webhook payload
- `event` -- the event type to subscribe to (see below)
- `secret` (optional) -- a shared secret used to sign the payload

### Supported Events

The `WebhookEvent` enum defines eight event types:

- `FlowCompleted` -- a flow finished successfully
- `FlowFailed` -- a flow finished with an error
- `StepCompleted` -- a single step completed
- `StepFailed` -- a single step failed
- `DagCompleted` -- a DAG execution completed
- `DagFailed` -- a DAG execution failed
- `RunCompleted` -- a full run completed
- `BuildCompleted` -- a module build completed

### Storage

Webhook subscriptions are stored in the configured backend (SQLite or Postgres). When the corresponding event fires, Automaton delivers the webhook payload to the registered URL.

---

## Section 6: Daemon Mode

Automaton can run as a long-lived daemon that processes scheduled and queued work.

### Starting the Daemon

```sh
automaton start
```

This command launches two components in the same process:

### Scheduler

The scheduler polls all enabled cron triggers on a fixed interval. When a trigger's schedule matches the current time, the scheduler enqueues a job for that trigger. The scheduler uses the same `RegistryBackend` as the rest of the system, so it works with both SQLite and Postgres.

### Worker

The worker dequeues jobs from the backend and executes them concurrently. It pulls jobs from the queue, processes them, and records results. The worker runs alongside the scheduler in the same process, sharing the same backend connection pool.

### Single-Process Design

Running the scheduler and worker in the same process simplifies deployment. There is no need for a separate job queue server or sidecar process. Both components coordinate through the shared `RegistryBackend`, which handles job state consistently regardless of the storage backend.

---

## Section 7: Security

### Secret Storage

Sensitive values (API keys, tokens, passwords) are encrypted at rest using AES-256-GCM. The encryption key is derived from the configured `AUTOMATON_SECRET` environment variable. This ensures secrets stored in the registry are not readable from the database alone.

The implementation lives in `automaton-core/src/secrets.rs` and uses the `aes-gcm` crate. Each secret is encrypted with a unique nonce and authenticated with a GCM tag.

### REST API Authentication

The REST API supports optional JWT authentication. When enabled, all API endpoints require a valid JWT token in the `Authorization` header. Configure the JWT issuer and secret through environment variables.

### Rate Limiting

See Section 4 above. Rate limiting prevents a single client or runaway process from exhausting server resources.

### Shell Execution

Automaton executes shell commands as part of automation workflows. Subprocesses are created with `kill_on_drop(true)`, which ensures that when a job times out or is cancelled, the child process is terminated and cannot become an orphan. This prevents runaway processes from accumulating on the system.

---

## Section 8: Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | For Postgres | none | Postgres connection string. Required when using the Postgres backend. |
| `MAX_CONCURRENT_REQUESTS` | No | `100` | Maximum concurrent REST API requests. Raise for high-traffic deployments. |
| `AUTOMATON_SECRET` | Recommended | none | Encryption key for secret storage. Used to derive the AES-256-GCM key. Must be set if you store secrets. |
| `AUTOMATON_WORK_DIR` | No | system temp | Working directory for module builds. Should have sufficient disk space and appropriate permissions. |
| `AUTOMATON_TEMP_DIR` | No | system temp | Temporary directory for compilation artifacts. Used during module compilation before results are copied to the work directory. |

### Usage Notes

- `DATABASE_URL` is only needed for Postgres. SQLite uses a file path configured separately or the default path.
- `AUTOMATON_SECRET` should be a long, random string. Changing it after secrets have been stored will make them undecryptable. Treat it like a master key.
- `AUTOMATON_WORK_DIR` and `AUTOMATON_TEMP_DIR` should point to directories with enough free space for your workloads. Build artifacts can be large for complex modules.
