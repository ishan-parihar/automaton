//! Worker daemon for Automaton.
//! Polls a job queue, compiles scripts, executes them, and stores results.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use automaton_build::BuildCache;
use automaton_registry::Registry;
use automaton_runtime::{Runtime, RuntimeConfig};
use tokio::task::JoinSet;

pub struct Worker {
    name: String,
    concurrency: usize,
    build_cache: Option<BuildCache>,
    data_dir: PathBuf,
    runtime: Runtime,
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    pub fn new(name: &str, concurrency: usize) -> Self {
        Self {
            name: name.to_string(),
            concurrency,
            build_cache: None,
            data_dir: PathBuf::from("."),
            runtime: Runtime::new(RuntimeConfig::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_build_cache(mut self, cache: BuildCache) -> Self {
        self.build_cache = Some(cache);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        self.shutdown.clone()
    }

    /// Signal the worker to shut down gracefully
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Run a module directly: compile (if needed) and execute
    pub async fn run_module(
        &self,
        name: &str,
        source: &str,
        manifest: &automaton_core::AutomationManifest,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if let Some(ref cache) = self.build_cache {
            let (_hash, binary_path) = cache
                .build_rust(name, source, manifest)
                .map_err(|e| format!("Build failed: {e}"))?;

            let timeout = manifest.timeout_ms;
            let retry = manifest.retry.as_ref();

            let result = if let Some(retry_cfg) = retry {
                self.runtime
                    .run_with_retry(&binary_path, input, retry_cfg, timeout)
                    .await
                    .map_err(|e| e.to_string())?
            } else {
                self.runtime
                    .run_binary(&binary_path, input, timeout)
                    .await
                    .map_err(|e| e.to_string())?
            };
            Ok(result)
        } else {
            Err("No build cache configured".to_string())
        }
    }

    /// Process a single job: dequeue, execute, complete.
    /// Returns true if a job was processed.
    async fn process_job(
        &self,
        registry: &Registry,
        poll_interval: Duration,
    ) -> bool {
        match registry.dequeue(&self.name) {
            Ok(Some(job)) => {
                let job_id = job["id"].as_i64().unwrap_or(0);
                let target = job["target_path"].as_str().unwrap_or("").to_string();
                let args = job.get("args").cloned().unwrap_or(serde_json::json!({}));

                tracing::info!(
                    worker = %self.name,
                    job_id,
                    target = %target,
                    "Processing job"
                );

                match registry.get(&target) {
                    Ok(Some(module)) => {
                        if module.built {
                            match self.run_module(&target, &module.source, &module.manifest, &args).await {
                                Ok(_output) => {
                                    registry.record_run(
                                        &uuid::Uuid::new_v4().to_string(),
                                        &target,
                                        &args,
                                    ).ok();
                                    tracing::info!(job_id, target = %target, "Job completed");
                                }
                                Err(e) => {
                                    tracing::error!(job_id, target = %target, error = %e, "Job failed");
                                }
                            }
                        } else {
                            tracing::warn!(job_id, target = %target, "Module not built");
                            if let Some(ref cache) = self.build_cache {
                                if let Ok((_hash, _path)) = cache.build_rust(&target, &module.source, &module.manifest) {
                                    registry.mark_built(&target).ok();
                                    match self.run_module(&target, &module.source, &module.manifest, &args).await {
                                        Ok(_output) => {
                                            registry.record_run(
                                                &uuid::Uuid::new_v4().to_string(),
                                                &target,
                                                &args,
                                            ).ok();
                                            tracing::info!(job_id, target = %target, "Built and completed");
                                        }
                                        Err(e) => tracing::error!(job_id, error = %e, "Run after build failed"),
                                    }
                                } else {
                                    tracing::error!(target = %target, "Build failed on-the-fly");
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(job_id, target = %target, "Module not found in registry");
                    }
                    Err(e) => {
                        tracing::error!(worker = %self.name, error = %e, "Registry error");
                    }
                }

                if let Err(e) = registry.complete_job(job_id) {
                    tracing::error!(job_id, error = %e, "Failed to complete job");
                }
                true
            }
            Ok(None) => {
                tokio::time::sleep(poll_interval).await;
                false
            }
            Err(e) => {
                tracing::error!(worker = %self.name, error = %e, "Dequeue error");
                tokio::time::sleep(poll_interval).await;
                false
            }
        }
    }

    /// Start the worker pull loop.
    /// When concurrency > 1, spawns N independent polling tasks
    /// each opening its own Registry handle (SQLite WAL supports concurrent access).
    pub async fn start(
        &self,
        registry: &Registry,
        poll_interval_ms: u64,
    ) {
        let concurrency = self.concurrency.max(1);
        tracing::info!(worker = %self.name, concurrency = concurrency, "Worker starting");

        if concurrency == 1 {
            let poll_interval = Duration::from_millis(poll_interval_ms);
            loop {
                if self.shutdown.load(Ordering::SeqCst) {
                    tracing::info!(worker = %self.name, "Worker shutting down");
                    break;
                }
                self.process_job(registry, poll_interval).await;
            }
        } else {
            self.run_concurrent(poll_interval_ms, concurrency).await;
        }
    }

    /// Concurrent mode: spawn N tasks, each with its own Registry handle.
    async fn run_concurrent(&self, poll_interval_ms: u64, concurrency: usize) {
        let shutdown = self.shutdown.clone();
        let name = self.name.clone();
        let data_dir = self.data_dir.clone();
        let build_cache = self.build_cache.clone();
        let runtime_config = RuntimeConfig::default();

        let mut set = JoinSet::new();

        for i in 0..concurrency {
            let shutdown = shutdown.clone();
            let worker_name = format!("{}-{}", name, i);
            let data_dir = data_dir.clone();
            let build_cache = build_cache.clone();
            let runtime = Runtime::new(runtime_config.clone());

            set.spawn(async move {
                let registry = match Registry::open(&data_dir) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(task = %worker_name, error = %e, "Failed to open registry");
                        return;
                    }
                };

                tracing::info!(task = %worker_name, "Worker task started");

                loop {
                    if shutdown.load(Ordering::SeqCst) {
                        tracing::info!(task = %worker_name, "Shutting down");
                        break;
                    }

                    match registry.dequeue(&worker_name) {
                        Ok(Some(job)) => {
                            let job_id = job["id"].as_i64().unwrap_or(0);
                            let target = job["target_path"].as_str().unwrap_or("").to_string();
                            let args = job.get("args").cloned().unwrap_or(serde_json::json!({}));

                            tracing::info!(task = %worker_name, job_id, target = %target, "Processing");

                            match registry.get(&target) {
                                Ok(Some(module)) => {
                                    if module.built {
                                        let result = build_cache.as_ref().and_then(|cache| {
                                            cache.build_rust(&target, &module.source, &module.manifest).ok()
                                        });
                                        if let Some((_hash, binary_path)) = result {
                                            let timeout = module.manifest.timeout_ms;
                                            let retry = module.manifest.retry.as_ref();
                                            let run_result = if let Some(rc) = retry {
                                                runtime.run_with_retry(&binary_path, &args, rc, timeout).await
                                            } else {
                                                runtime.run_binary(&binary_path, &args, timeout).await
                                            };
                                            if run_result.is_ok() {
                                                let _ = registry.record_run(
                                                    &uuid::Uuid::new_v4().to_string(),
                                                    &target,
                                                    &args,
                                                );
                                                tracing::info!(job_id, "Completed");
                                            } else {
                                                tracing::error!(job_id, "Failed");
                                            }
                                        }
                                    } else {
                                        tracing::warn!(target = %target, "Not built");
                                    }
                                }
                                Ok(None) => tracing::warn!(target = %target, "Not found"),
                                Err(e) => tracing::error!(error = %e, "Registry error"),
                            }

                            if let Err(e) = registry.complete_job(job_id) {
                                tracing::error!(job_id, error = %e, "Failed to complete job");
                            }
                        }
                        Ok(None) => {
                            tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
                        }
                        Err(e) => {
                            tracing::error!(task = %worker_name, error = %e, "Dequeue error");
                            tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
                        }
                    }
                }
            });
        }

        while let Some(result) = set.join_next().await {
            if let Err(e) = result {
                tracing::error!(error = %e, "Worker task failed");
            }
        }
    }
}
