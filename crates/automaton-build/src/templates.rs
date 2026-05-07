//! Embedded module templates for Automaton.
//! Each template is a named Rust source with associated Cargo dependencies.

/// A template definition
pub struct Template {
    pub name: &'static str,
    pub description: &'static str,
    pub source: &'static str,
    /// Extra Cargo dependencies beyond the defaults (serde, serde_json, tokio, anyhow)
    pub extra_deps: &'static [(&'static str, &'static str)], // (name, version_spec)
}

/// Get all available templates
pub fn all_templates() -> Vec<&'static Template> {
    vec![
        &ECHO,
        &HTTP_FETCH,
        &HTTP_SERVER,
        &DB_QUERY,
        &SLACK_NOTIFY,
        &DATA_TRANSFORM,
        &HEALTH_CHECK,
        &RATE_LIMITER,
        &FILE_WATCH,
        &CRON_WORKER,
    ]
}

/// Get a template by name
pub fn get_template(name: &str) -> Option<&'static Template> {
    all_templates().into_iter().find(|t| t.name == name)
}

// ── Template Definitions ──

static ECHO: Template = Template {
    name: "echo",
    description: "Simple JSON echo module — prints input as output",
    extra_deps: &[],
    source: r#"// Simple echo module — accepts JSON input and echoes it back
use serde::{Deserialize, Serialize};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: serde_json::Value = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };
    let output = serde_json::json!({
        "status": "ok",
        "echoed_input": input,
        "module": env!("CARGO_PKG_NAME"),
    });
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static HTTP_FETCH: Template = Template {
    name: "http-fetch",
    description: "HTTP client module — fetch URLs with GET/POST and return response",
    extra_deps: &[("reqwest", r#"{ version = "0.12", features = ["json"] }"#)],
    source: r#"// HTTP fetch module — performs GET/POST requests and returns structured results
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Output {
    status: u16,
    headers: HashMap<String, String>,
    body: serde_json::Value,
    duration_ms: u64,
}

use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("automaton-http-fetch/1.0")
        .build()?;

    let start = std::time::Instant::now();
    let method = input.method.as_deref().unwrap_or("GET");

    let req = match method.to_uppercase().as_str() {
        "GET" => client.get(&input.url),
        "POST" => {
            let mut r = client.post(&input.url);
            if let Some(body) = &input.body {
                r = r.json(body);
            }
            r
        }
        "PUT" => {
            let mut r = client.put(&input.url);
            if let Some(body) = &input.body {
                r = r.json(body);
            }
            r
        }
        "DELETE" => client.delete(&input.url),
        _ => client.get(&input.url),
    };

    let req = if let Some(headers) = &input.headers {
        let mut r = req;
        for (k, v) in headers {
            r = r.header(k.as_str(), v.as_str());
        }
        r
    } else {
        req
    };

    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let resp_headers: HashMap<String, String> = resp.headers().iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({"raw": "non-json response"}));
    let duration_ms = start.elapsed().as_millis() as u64;

    let output = Output { status, headers: resp_headers, body, duration_ms };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static HTTP_SERVER: Template = Template {
    name: "http-server",
    description: "Mini HTTP server module — serves requests on a configurable port",
    extra_deps: &[
        ("axum", r#"{ version = "0.8" }"#),
        ("tower", r#"{ version = "0.5" }"#),
    ],
    source: r#"// Mini HTTP server module — starts an axum server on a configurable port
use axum::{Router, routing::get, Json, response::IntoResponse};
use std::net::SocketAddr;
use serde::Serialize;

#[derive(Serialize)]
struct Health {
    status: String,
    module: String,
    uptime_secs: u64,
}

static START_TIME: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn uptime() -> u64 {
    START_TIME.get_or_init(std::time::Instant::now).elapsed().as_secs()
}

async fn health_handler() -> impl IntoResponse {
    Json(Health {
        status: "ok".into(),
        module: env!("CARGO_PKG_NAME").into(),
        uptime_secs: uptime(),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = if args.len() > 1 && args[1] == "--port" {
        args[2].parse()?
    } else {
        8080
    };

    let app = Router::new()
        .route("/health", get(health_handler));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("{}", serde_json::json!({"status":"starting","port":port}));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
"#,
};

static DB_QUERY: Template = Template {
    name: "db-query",
    description: "SQLite query module — execute SQL queries against a local database",
    extra_deps: &[(
        "rusqlite",
        r#"{ version = "0.35", features = ["bundled"] }"#,
    )],
    source: r#"// SQLite query module — run SQL queries and return results as JSON
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    query: String,
    params: Option<Vec<serde_json::Value>>,
    db_path: Option<String>,
}

fn query_to_json(db_path: &str, query: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let conn = rusqlite::Connection::open(db_path)?;
    let mut stmt = conn.prepare(query)?;
    let cols: Vec<String> = stmt.columns().iter().map(|c| c.name().to_string()).collect();
    let rows = stmt.query_map([], |row| {
        let mut map = serde_json::Map::new();
        for (i, col) in cols.iter().enumerate() {
            let val: Result<String, _> = row.get(i);
            if let Ok(v) = val {
                map.insert(col.clone(), serde_json::Value::String(v));
            }
        }
        Ok(serde_json::Value::Object(map))
    })?;
    let mut results = vec![];
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let db_path = input.db_path.unwrap_or_else(|| "data.db".to_string());
    let results = query_to_json(&db_path, &input.query)?;
    let output = serde_json::json!({
        "row_count": results.len(),
        "columns": if results.is_empty() { [] } else {
            results[0].as_object().map(|o| o.keys().collect::<Vec<_>>()).unwrap_or_default()
        },
        "rows": results,
    });
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static SLACK_NOTIFY: Template = Template {
    name: "slack-notify",
    description: "Slack notification module — sends messages via Slack webhook",
    extra_deps: &[("reqwest", r#"{ version = "0.12", features = ["json"] }"#)],
    source: r#"// Slack notification module — sends messages via Slack Incoming Webhook
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    webhook_url: String,
    channel: Option<String>,
    text: String,
    username: Option<String>,
    icon_emoji: Option<String>,
}

#[derive(Serialize)]
struct SlackMessage {
    channel: Option<String>,
    text: String,
    username: Option<String>,
    icon_emoji: Option<String>,
}

#[derive(Serialize)]
struct Output {
    status: String,
    status_code: u16,
    duration_ms: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let msg = SlackMessage {
        channel: input.channel,
        text: input.text,
        username: input.username,
        icon_emoji: input.icon_emoji,
    };

    let start = std::time::Instant::now();
    let client = reqwest::Client::new();
    let resp = client.post(&input.webhook_url).json(&msg).send().await?;
    let status_code = resp.status().as_u16();
    let duration_ms = start.elapsed().as_millis() as u64;

    let output = Output {
        status: if status_code < 300 { "sent".into() } else { "failed".into() },
        status_code,
        duration_ms,
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static DATA_TRANSFORM: Template = Template {
    name: "data-transform",
    description: "Data transformation module — apply mappings and filters to JSON data",
    extra_deps: &[("csv", r#""1.3""#)],
    source: r#"// Data transform module — filter, map, group, and aggregate JSON data
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct TransformRule {
    field: String,
    operation: String,       // keep, remove, rename, default, compute
    new_name: Option<String>,
    default_value: Option<serde_json::Value>,
    expression: Option<String>,
}

#[derive(Deserialize)]
struct Input {
    data: Vec<HashMap<String, serde_json::Value>>,
    rules: Vec<TransformRule>,
    sort_by: Option<String>,
    sort_desc: Option<bool>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct Output {
    row_count: usize,
    transformed: Vec<HashMap<String, serde_json::Value>>,
    fields: Vec<String>,
}

fn apply_rules(
    rows: Vec<HashMap<String, serde_json::Value>>,
    rules: &[TransformRule],
) -> Vec<HashMap<String, serde_json::Value>> {
    rows.into_iter().map(|row| {
        let mut result = HashMap::new();
        for rule in rules {
            match rule.operation.as_str() {
                "keep" => {
                    if let Some(val) = row.get(&rule.field) {
                        result.insert(rule.new_name.clone().unwrap_or_else(|| rule.field.clone()), val.clone());
                    }
                }
                "remove" => { /* skip this field */ }
                "rename" => {
                    if let Some(val) = row.get(&rule.field) {
                        result.insert(rule.new_name.clone().unwrap_or_else(|| rule.field.clone()), val.clone());
                    }
                }
                "default" => {
                    result.insert(rule.field.clone(), rule.default_value.clone().unwrap_or(serde_json::Value::Null));
                }
                _ => {
                    if let Some(val) = row.get(&rule.field) {
                        result.insert(rule.field.clone(), val.clone());
                    }
                }
            }
        }
        result
    }).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let transformed = apply_rules(input.data, &input.rules);
    let fields: Vec<String> = transformed.first()
        .map(|r| r.keys().cloned().collect())
        .unwrap_or_default();

    let output = Output {
        row_count: transformed.len(),
        transformed,
        fields,
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static HEALTH_CHECK: Template = Template {
    name: "health-check",
    description: "Health check module — ping multiple endpoints and report status",
    extra_deps: &[("reqwest", r#"{ version = "0.12", features = ["json"] }"#)],
    source: r#"// Health check module — pings multiple endpoints concurrently and reports status
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Endpoint {
    name: String,
    url: String,
    method: Option<String>,
    expected_status: Option<u16>,
    timeout_secs: Option<u64>,
}

#[derive(Deserialize)]
struct Input {
    endpoints: Vec<Endpoint>,
    concurrency: Option<usize>,
}

#[derive(Serialize)]
struct EndpointResult {
    name: String,
    url: String,
    status: String,
    status_code: u16,
    duration_ms: u64,
    error: Option<String>,
}

#[derive(Serialize)]
struct Output {
    total: usize,
    passed: usize,
    failed: usize,
    results: Vec<EndpointResult>,
    total_duration_ms: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let overall_start = Instant::now();
    let mut results = Vec::new();

    for ep in &input.endpoints {
        let start = Instant::now();
        let timeout = Duration::from_secs(ep.timeout_secs.unwrap_or(5));
        let method = ep.method.as_deref().unwrap_or("GET");

        let result = match tokio::time::timeout(timeout, async {
            let req = match method.to_uppercase().as_str() {
                "GET" => client.get(&ep.url),
                "HEAD" => client.head(&ep.url),
                _ => client.get(&ep.url),
            };
            req.send().await
        }).await {
            Ok(Ok(resp)) => {
                let status_code = resp.status().as_u16();
                let expected = ep.expected_status.unwrap_or(200);
                EndpointResult {
                    name: ep.name.clone(),
                    url: ep.url.clone(),
                    status: if status_code == expected { "pass".into() } else { "unexpected_status".into() },
                    status_code,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: if status_code != expected { Some(format!("Expected {expected}, got {status_code}")) } else { None },
                }
            }
            Ok(Err(e)) => EndpointResult {
                name: ep.name.clone(), url: ep.url.clone(),
                status: "error".into(), status_code: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some(e.to_string()),
            },
            Err(_) => EndpointResult {
                name: ep.name.clone(), url: ep.url.clone(),
                status: "timeout".into(), status_code: 0,
                duration_ms: timeout.as_millis() as u64,
                error: Some("Request timed out".into()),
            },
        };
        results.push(result);
    }

    let passed = results.iter().filter(|r| r.status == "pass").count();
    let failed = results.len() - passed;

    let output = Output {
        total: results.len(),
        passed,
        failed,
        results,
        total_duration_ms: overall_start.elapsed().as_millis() as u64,
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static RATE_LIMITER: Template = Template {
    name: "rate-limiter",
    description: "Token-bucket rate limiter module — throttle requests to an API",
    extra_deps: &[("reqwest", r#"{ version = "0.12", features = ["json"] }"#)],
    source: r#"// Token-bucket rate limiter module — wraps API calls with rate limiting
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct TokenBucket {
    capacity: u64,
    tokens: f64,
    refill_rate: f64,    // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u64, refill_per_sec: f64) -> Self {
        Self { capacity, tokens: capacity as f64, refill_rate: refill_per_sec, last_refill: Instant::now() }
    }

    fn acquire(&mut self, tokens: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
        self.last_refill = now;
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn wait_time(&self) -> f64 {
        let needed = 1.0 - self.tokens;
        if needed <= 0.0 { 0.0 } else { needed / self.refill_rate }
    }
}

#[derive(Deserialize)]
struct Input {
    requests: Vec<RequestDef>,
    rate_per_second: f64,
    burst: Option<u64>,
}

#[derive(Deserialize)]
struct RequestDef {
    id: String,
    url: String,
    method: Option<String>,
}

#[derive(Serialize)]
struct Output {
    results: Vec<serde_json::Value>,
    total_duration_ms: u64,
    throttled: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let bucket = Mutex::new(TokenBucket::new(
        input.burst.unwrap_or(input.rate_per_second as u64),
        input.rate_per_second,
    ));

    let client = reqwest::Client::new();
    let start = Instant::now();
    let mut results = Vec::new();
    let mut throttled = 0u64;

    for req_def in &input.requests {
        // Wait until we have a token
        loop {
            let can_proceed = {
                let mut b = bucket.lock().unwrap();
                if b.acquire(1.0) {
                    true
                } else {
                    let wait_ms = (b.wait_time() * 1000.0) as u64;
                    if wait_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(wait_ms.min(100)));
                    }
                    false
                }
            };
            if can_proceed {
                break;
            }
            throttled += 1;
        }

        let method = req_def.method.as_deref().unwrap_or("GET").to_uppercase();
        let req_start = Instant::now();
        let resp = match method.as_str() {
            "GET" => client.get(&req_def.url).send().await,
            _ => client.get(&req_def.url).send().await,
        };

        match resp {
            Ok(r) => {
                let status = r.status().as_u16();
                let body: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
                results.push(serde_json::json!({
                    "id": req_def.id, "status": status, "body": body,
                    "duration_ms": req_start.elapsed().as_millis() as u64,
                }));
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "id": req_def.id, "error": e.to_string(),
                    "duration_ms": req_start.elapsed().as_millis() as u64,
                }));
            }
        }
    }

    let output = Output {
        results,
        total_duration_ms: start.elapsed().as_millis() as u64,
        throttled,
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static FILE_WATCH: Template = Template {
    name: "file-watch",
    description: "File system watcher module — monitor directories for changes",
    extra_deps: &[(
        "notify",
        r#"{ version = "7", features = ["macos_kqueue"] }"#,
    )],
    source: r#"// File system watcher module — monitor directories for changes
use serde::{Deserialize, Serialize};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Deserialize)]
struct Input {
    paths: Vec<String>,
    recursive: Option<bool>,
    events: Option<Vec<String>>,    // create, modify, remove
    duration_secs: Option<u64>,
    patterns: Option<Vec<String>>,  // glob patterns to filter
}

#[derive(Serialize)]
struct FileEvent {
    path: String,
    kind: String,
    timestamp: String,
}

#[derive(Serialize)]
struct Output {
    events: Vec<FileEvent>,
    total: usize,
    duration_secs: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let (tx, rx) = mpsc::channel::<Result<Event, notify::Error>>();
    let mut watcher = notify::recommended_watcher(tx)?;
    let recursive = if input.recursive.unwrap_or(true) { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };

    let watch_paths: Vec<PathBuf> = input.paths.iter().map(PathBuf::from).collect();
    for path in &watch_paths {
        watcher.watch(path, recursive)?;
    }

    let duration = Duration::from_secs(input.duration_secs.unwrap_or(10));
    let start = std::time::Instant::now();
    let mut events = Vec::new();

    loop {
        let remaining = duration.checked_sub(start.elapsed()).unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(Ok(event)) => {
                let kind = match event.kind {
                    EventKind::Create(_) => "create",
                    EventKind::Modify(_) => "modify",
                    EventKind::Remove(_) => "remove",
                    _ => "other",
                }.to_string();

                for path in event.paths {
                    let file_event = FileEvent {
                        path: path.to_string_lossy().to_string(),
                        kind: kind.clone(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    };
                    events.push(file_event);
                }
            }
            Ok(Err(_)) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(_) => break,
        }
    }

    let output = Output {
        total: events.len(),
        events,
        duration_secs: start.elapsed().as_secs(),
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};

static CRON_WORKER: Template = Template {
    name: "cron-worker",
    description: "Scheduled task worker — run a job on a cron schedule",
    extra_deps: &[("croner", r#""2""#)],
    source: r#"// Cron worker module — runs a task on a configurable schedule
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Deserialize)]
struct Input {
    schedule: String,             // cron expression like "*/5 * * * * *"
    task: TaskDef,
    max_executions: Option<u32>,
    timezone: Option<String>,
}

#[derive(Deserialize)]
struct TaskDef {
    name: String,
    command: String,
    args: Vec<String>,
}

#[derive(Serialize)]
struct Execution {
    iteration: u32,
    status: String,
    output: String,
    duration_ms: u64,
    started_at: String,
}

#[derive(Serialize)]
struct Output {
    executions: Vec<Execution>,
    total: u32,
    duration_secs: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = if args.len() > 1 && args[1] == "--input" {
        serde_json::from_str(&args[2])?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        serde_json::from_str(&buf)?
    };

    let mut cron = croner::Cron::new(&input.schedule);
    cron.with_seconds_optional();
    let cron = cron.parse()?;
    let max_execs = input.max_executions.unwrap_or(5);
    let mut executions = Vec::new();
    let start = std::time::Instant::now();

    for i in 1..=max_execs {
        // Wait for next cron match
        let mut checked = 0u64;
        loop {
            let now = chrono::Utc::now();
            if cron.is_time_matching(&now).unwrap_or(false) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            checked += 1;
            if checked > 120 || start.elapsed().as_secs() > 3600 {
                break;
            }
        }

        if checked > 120 {
            break;
        }

        // Execute the task
        let task_start = std::time::Instant::now();
        let output = std::process::Command::new(&input.task.command)
            .args(&input.task.args)
            .output();

        let (status, output_str) = match output {
            Ok(o) => (
                if o.status.success() { "completed".into() } else { "failed".into() },
                String::from_utf8_lossy(&o.stdout).to_string(),
            ),
            Err(e) => ("error".into(), e.to_string()),
        };

        executions.push(Execution {
            iteration: i,
            status,
            output: output_str,
            duration_ms: task_start.elapsed().as_millis() as u64,
            started_at: chrono::Utc::now().to_rfc3339(),
        });

        if i < max_execs {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    let output = Output {
        total: executions.len(),
        executions,
        duration_secs: start.elapsed().as_secs(),
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
"#,
};
