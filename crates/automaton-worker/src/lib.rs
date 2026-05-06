//! Worker daemon for Automaton.
//! Polls a job queue, compiles scripts, executes them, and stores results.


use automaton_build::BuildCache;
use automaton_runtime::{Runtime, RuntimeConfig};

pub struct Worker {
    name: String,
    concurrency: usize,
    build_cache: Option<BuildCache>,
    runtime: Runtime,
}

impl Worker {
    pub fn new(name: &str, concurrency: usize) -> Self {
        Self {
            name: name.to_string(),
            concurrency,
            build_cache: None,
            runtime: Runtime::new(RuntimeConfig::default()),
        }
    }

    pub fn with_build_cache(mut self, cache: BuildCache) -> Self {
        self.build_cache = Some(cache);
        self
    }

    pub fn name(&self) -> &str { &self.name }

    /// Run a module directly: compile (if needed) and execute
    pub async fn run_module(
        &self,
        name: &str,
        source: &str,
        manifest: &automaton_core::AutomationManifest,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // Compile if we have a build cache
        if let Some(ref cache) = self.build_cache {
            let (_hash, binary_path) = cache.build_rust(name, source, manifest)
                .map_err(|e| format!("Build failed: {e}"))?;

            let timeout = manifest.timeout_ms;
            let retry = manifest.retry.as_ref();

            // Execute with retry
            let result = if let Some(retry_cfg) = retry {
                self.runtime.run_with_retry(&binary_path, input, retry_cfg, timeout).await
                    .map_err(|e| e.to_string())?
            } else {
                self.runtime.run_binary(&binary_path, input, timeout).await
                    .map_err(|e| e.to_string())?
            };

            Ok(result)
        } else {
            Err("No build cache configured".to_string())
        }
    }
}
