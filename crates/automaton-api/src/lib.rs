//! REST API server for Automaton — Axum-based HTTP interface.
//! Exposes all CRUD operations via JSON API, backed by automaton-postgres (sqlx).

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;
use tokio::sync::Semaphore;

use automaton_core::DepRef;

/// JWT auth middleware. If `AUTOMATON_JWT_SECRET` env var is set, validates
/// Bearer tokens on all `/api/*` routes. If unset, allows all requests.

async fn auth_middleware(
    request: axum::http::Request<Body>,
    next: middleware::Next,
) -> Result<Response, StatusCode> {
    let jwt_secret = std::env::var("AUTOMATON_JWT_SECRET").unwrap_or_default();
    if jwt_secret.is_empty() {
        return Ok(next.run(request).await);
    }

    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string());
    let token = auth_header.ok_or(StatusCode::UNAUTHORIZED)?;

    use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
    let key = DecodingKey::from_secret(jwt_secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    decode::<serde_json::Value>(&token, &key, &validation)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(next.run(request).await)
}

/// Rate limiting middleware using a semaphore to cap concurrent in-flight requests.
/// Returns 429 Too Many Requests when the limit is exceeded.
async fn rate_limit_middleware(
    State(semaphore): State<Arc<Semaphore>>,
    request: axum::http::Request<Body>,
    next: middleware::Next,
) -> Response {
    let _permit = match semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "rate_limit_exceeded",
                    "message": "Too many concurrent requests"
                })),
            )
                .into_response();
        }
    };
    next.run(request).await
}

/// Create a router with all API endpoints, backed by the given DB and build cache dir.
pub fn create_router(
    db: Arc<automaton_postgres::AutomatonDb>,
    data_dir: PathBuf,
) -> Router {
    let build_cache_dir = data_dir.join("builds");
    let app_state = AppState { db, build_cache_dir, data_dir };

    let max_concurrent = std::env::var("MAX_CONCURRENT_REQUESTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let semaphore = Arc::new(Semaphore::new(max_concurrent));

    Router::new()
        .route("/health", get(health_handler))
        .route("/api/scripts", get(list_scripts).post(create_script))
        .route("/api/scripts/:path", get(get_script))
        .route("/api/scripts/:path/build", post(build_script))
        .route("/api/scripts/:path/run", post(run_script))
        .route("/api/jobs", get(list_jobs).post(enqueue_job))
        .route("/api/runs", get(list_runs))
        .route("/api/variables", get(list_variables).post(set_variable))
        .route("/api/variables/:path", get(get_variable))
        .route("/api/resources", get(list_resources).post(set_resource))
        .route("/api/resources/:path", get(get_resource))
        .route("/api/webhooks/:trigger_id", post(webhook_handler))
        .route("/api/events/:trigger_id", post(event_handler))
        .route("/api/triggers", get(list_triggers).post(create_trigger))
        .route("/api/graph/nodes", post(add_node))
        .route("/api/graph/edges", post(add_edge))
        .route_layer(middleware::from_fn(auth_middleware))
        .layer(middleware::from_fn_with_state(
            semaphore,
            rate_limit_middleware,
        ))
        .with_state(Arc::new(app_state))
}

/// Start the API server on the given address.
pub async fn serve(
    addr: &str,
    database_url: &str,
    data_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let db = automaton_postgres::AutomatonDb::connect(database_url).await?;
    let db = Arc::new(db);
    let data_dir = data_dir.unwrap_or_else(|| PathBuf::from("./data"));
    let app = create_router(db, data_dir);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Automaton API server starting on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

struct AppState {
    db: Arc<automaton_postgres::AutomatonDb>,
    build_cache_dir: PathBuf,
    data_dir: PathBuf,
}

// ── Request/Response types ──

fn ok_json(v: Value) -> Response {
    (StatusCode::OK, Json(v)).into_response()
}

fn err_msg(code: StatusCode, msg: &str) -> Response {
    (code, Json(serde_json::json!({"error": msg}))).into_response()
}

// ── Health ──

async fn health_handler() -> Response {
    ok_json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// ── Scripts ──

async fn create_script(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let path = body["path"].as_str().unwrap_or_default().to_string();
    let source = body["source"].as_str().unwrap_or_default().to_string();
    if path.is_empty() || source.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "path and source are required");
    }
    let manifest = serde_json::json!({
        "name": path,
        "version": body.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0"),
        "summary": body.get("summary"),
        "timeout_ms": body.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(30000),
    });
    let deps: Vec<DepRef> = body.get("depends_on")
        .and_then(|d| d.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(DepRef::new).collect())
        .unwrap_or_default();

    match state.db.register_script(&path, &source, "0.1.0", &manifest, &deps).await {
        Ok(hash) => ok_json(serde_json::json!({"hash": hash, "path": path})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_scripts(State(state): State<Arc<AppState>>) -> Response {
    match state.db.list_scripts().await {
        Ok(scripts) => ok_json(serde_json::json!({"scripts": scripts})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn get_script(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Response {
    match state.db.get_script(&path).await {
        Ok(Some(script)) => ok_json(script),
        Ok(None) => err_msg(StatusCode::NOT_FOUND, "Script not found"),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn build_script(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Response {
    // Look up the script source
    let script = match state.db.get_script(&path).await {
        Ok(Some(s)) => s,
        Ok(None) => return err_msg(StatusCode::NOT_FOUND, "Script not found"),
        Err(e) => return err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    let source = script.get("source").and_then(|v| v.as_str()).unwrap_or("");
    if source.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "Script has no source code");
    }

    // Build manifest from script metadata
    let manifest = automaton_core::AutomationManifest {
        name: path.clone(),
        version: script.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0").to_string(),
        entry: "main".to_string(),
        summary: None,
        description: None,
        timeout_ms: 30_000,
        retry: None,
        permissions: vec![],
        depends_on: vec![],
        resources: vec![],
        tags: vec![],
        require_approval: false,
        inputs_schema: automaton_core::SchemaMode::Auto,
        outputs_schema: automaton_core::SchemaMode::Auto,
    };

    // Build using cache
    let build_cache = automaton_build::BuildCache::new(&state.data_dir);
    match build_cache.build_rust(&path, source, &manifest) {
        Ok((hash, binary_path)) => {
            ok_json(serde_json::json!({
                "status": "built",
                "hash": hash,
                "binary": binary_path.to_string_lossy(),
            }))
        }
        Err(e) => {
            // Parse diagnostics for structured error feedback
            let diagnostics = automaton_build::BuildCache::diagnose(&e);
            ok_json(serde_json::json!({
                "status": "failed",
                "error": e,
                "diagnostics": diagnostics,
            }))
        }
    }
}

async fn run_script(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    Json(args): Json<Value>,
) -> Response {
    let args = if args.is_null() { serde_json::json!({}) } else { args };
    match state.db.enqueue("script", &path, &args).await {
        Ok(job_id) => ok_json(serde_json::json!({"status": "queued", "job_id": job_id})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── Jobs ──

async fn enqueue_job(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let target = body["target_path"].as_str().unwrap_or_default();
    let kind = body.get("kind").and_then(|v| v.as_str()).unwrap_or("script");
    let args = body.get("args").cloned().unwrap_or(serde_json::json!({}));
    if target.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "target_path is required");
    }
    match state.db.enqueue(kind, target, &args).await {
        Ok(job_id) => ok_json(serde_json::json!({"job_id": job_id, "status": "queued"})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Value>,
) -> Response {
    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);
    match state.db.list_jobs(limit).await {
        Ok(jobs) => ok_json(serde_json::json!({"jobs": jobs})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── Runs ──

async fn list_runs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Value>,
) -> Response {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);
    match state.db.get_runs(path, limit).await {
        Ok(runs) => ok_json(serde_json::json!({"runs": runs})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── Variables ──

async fn set_variable(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let path = body["path"].as_str().unwrap_or_default();
    let value = body["value"].as_str().unwrap_or_default();
    let is_secret = body.get("is_secret").and_then(|v| v.as_bool()).unwrap_or(true);
    if path.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "path is required");
    }
    match state.db.set_variable(path, value, is_secret).await {
        Ok(_) => ok_json(serde_json::json!({"status": "stored", "path": path})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn get_variable(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Response {
    match state.db.get_variable(&path).await {
        Ok(Some(val)) => ok_json(serde_json::json!({"path": path, "value": val})),
        Ok(None) => err_msg(StatusCode::NOT_FOUND, "Variable not found"),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_variables(State(state): State<Arc<AppState>>) -> Response {
    match state.db.list_variables().await {
        Ok(vars) => ok_json(serde_json::json!({"variables": vars})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── Resources ──

async fn set_resource(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let path = body["path"].as_str().unwrap_or_default();
    let rtype = body["resource_type"].as_str().unwrap_or_default();
    let value = body.get("value").cloned().unwrap_or(serde_json::json!({}));
    if path.is_empty() || rtype.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "path and resource_type are required");
    }
    match state.db.set_resource(path, rtype, &value).await {
        Ok(_) => ok_json(serde_json::json!({"status": "stored", "path": path})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn get_resource(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Response {
    match state.db.get_resource(&path).await {
        Ok(Some(res)) => ok_json(res),
        Ok(None) => err_msg(StatusCode::NOT_FOUND, "Resource not found"),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_resources(State(state): State<Arc<AppState>>) -> Response {
    match state.db.list_resources(None).await {
        Ok(resources) => ok_json(serde_json::json!({"resources": resources})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── Triggers ──

async fn create_trigger(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let target = body["target_path"].as_str().unwrap_or_default();
    let ttype = body.get("trigger_type").and_then(|v| v.as_str()).unwrap_or("cron");
    let config = body.get("config").cloned().unwrap_or(serde_json::json!({}));
    if target.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "target_path is required");
    }
    match state.db.create_trigger(target, false, ttype, &config).await {
        Ok(id) => ok_json(serde_json::json!({"id": id, "status": "created"})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Receive an incoming event and enqueue a job for the matching event trigger.
/// Validates that the trigger is an Event type. The body is passed as args.
async fn event_handler(
    State(state): State<Arc<AppState>>,
    Path(trigger_id): Path<String>,
    Json(args): Json<serde_json::Value>,
) -> Response {
    let trigger = match state.db.get_trigger_by_id(&trigger_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return err_msg(StatusCode::NOT_FOUND, "Trigger not found"),
        Err(e) => return err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    if trigger.get("enabled") != Some(&serde_json::json!(true)) {
        return err_msg(StatusCode::GONE, "Trigger is disabled");
    }
    if trigger.get("trigger_type").and_then(|v| v.as_str()) != Some("event") {
        return err_msg(StatusCode::BAD_REQUEST, "Not an event trigger");
    }

    // Verify event_source match if configured on the trigger
    let expected_source = trigger.get("config")
        .and_then(|c| c.get("event_source"))
        .and_then(|v| v.as_str());
    if let Some(expected) = expected_source {
        let received = args.get("event_source").and_then(|v| v.as_str()).unwrap_or("");
        if received != expected {
            return err_msg(StatusCode::BAD_REQUEST, "Event source mismatch");
        }
    }

    let target_path = trigger.get("target_path").and_then(|v| v.as_str()).unwrap_or("");
    let target_is_flow = trigger.get("target_is_flow").and_then(|v| v.as_bool()).unwrap_or(false);
    let kind = if target_is_flow { "flow" } else { "script" };

    match state.db.enqueue(kind, target_path, &args).await {
        Ok(job_id) => ok_json(serde_json::json!({
            "status": "event_queued",
            "job_id": job_id,
            "target": target_path,
        })),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Receive an incoming webhook event and enqueue a job for the matching trigger.
/// The body is parsed as JSON and passed as args to the target automation.
/// If the trigger's config has `webhook_secret`, pass it as `webhook_secret` in the JSON body.
async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    Path(trigger_id): Path<String>,
    Json(args): Json<serde_json::Value>,
) -> Response {
    // Look up trigger by ID
    let trigger = match state.db.get_trigger_by_id(&trigger_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return err_msg(StatusCode::NOT_FOUND, "Trigger not found"),
        Err(e) => return err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    if trigger.get("enabled") != Some(&serde_json::json!(true)) {
        return err_msg(StatusCode::GONE, "Trigger is disabled");
    }

    // Verify webhook secret if configured
    let expected_secret = trigger.get("config")
        .and_then(|c| c.get("webhook_secret"))
        .and_then(|v| v.as_str());
    if let Some(expected) = expected_secret {
        let received = args.get("webhook_secret").and_then(|v| v.as_str()).unwrap_or("");
        if received != expected {
            return err_msg(StatusCode::UNAUTHORIZED, "Invalid webhook secret");
        }
    }

    let target_path = trigger.get("target_path").and_then(|v| v.as_str()).unwrap_or("");
    let target_is_flow = trigger.get("target_is_flow").and_then(|v| v.as_bool()).unwrap_or(false);
    let kind = if target_is_flow { "flow" } else { "script" };

    match state.db.enqueue(kind, target_path, &args).await {
        Ok(job_id) => ok_json(serde_json::json!({
            "status": "queued",
            "job_id": job_id,
            "target": target_path,
        })),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_triggers(State(state): State<Arc<AppState>>) -> Response {
    // Return all triggers regardless of type
    let cron = state.db.get_enabled_triggers("cron").await.unwrap_or_default();
    let webhook = state.db.get_enabled_triggers("webhook").await.unwrap_or_default();
    let event = state.db.get_enabled_triggers("event").await.unwrap_or_default();

    let mut all = cron;
    all.extend(webhook);
    all.extend(event);
    all.sort_by(|a, b| {
        a.get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("created_at").and_then(|v| v.as_str()).unwrap_or(""))
    });

    ok_json(serde_json::json!({"triggers": all}))
}

// ── Graph ──

async fn add_node(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let kind = body["kind"].as_str().unwrap_or_default();
    let name = body["name"].as_str().unwrap_or_default();
    let props = body.get("properties").cloned().unwrap_or(serde_json::json!({}));
    if kind.is_empty() || name.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "kind and name are required");
    }
    match state.db.add_node(kind, name, &props).await {
        Ok(id) => ok_json(serde_json::json!({"id": id})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn add_edge(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let source = body["source"].as_str().unwrap_or_default();
    let target = body["target"].as_str().unwrap_or_default();
    let kind = body["kind"].as_str().unwrap_or_default();
    if source.is_empty() || target.is_empty() || kind.is_empty() {
        return err_msg(StatusCode::BAD_REQUEST, "source, target, and kind are required");
    }
    match state.db.add_edge(source, target, kind).await {
        Ok(id) => ok_json(serde_json::json!({"id": id})),
        Err(e) => err_msg(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
