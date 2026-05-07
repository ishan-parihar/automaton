use automaton_core::*;

/// Local execution runtime.
/// Runs automation modules as child processes and manages their lifecycle.
pub struct Runtime {
    #[allow(dead_code)]
    config: RuntimeConfig,
}

/// Configuration for the runtime
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// Working directory for module execution
    pub work_dir: std::path::PathBuf,
    /// Default timeout in milliseconds
    pub default_timeout_ms: u64,
    /// Maximum concurrent executions
    pub max_concurrency: usize,
    /// Directory for temporary execution artifacts
    pub temp_dir: std::path::PathBuf,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            work_dir: std::path::PathBuf::from(".automaton/work"),
            default_timeout_ms: 30_000,
            max_concurrency: 4,
            temp_dir: std::path::PathBuf::from(".automaton/tmp"),
        }
    }
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        std::fs::create_dir_all(&config.work_dir).ok();
        std::fs::create_dir_all(&config.temp_dir).ok();
        Self { config }
    }

    /// Run a command-based module (for modules compiled as standalone binaries).
    pub async fn run_binary(
        &self,
        binary_path: &std::path::Path,
        input: &serde_json::Value,
        timeout_ms: u64,
    ) -> Result<serde_json::Value> {
        let input_str = serde_json::to_string(input)?;
        let timeout = std::time::Duration::from_millis(timeout_ms);

        let child = tokio::process::Command::new(binary_path)
            .arg("--input")
            .arg(&input_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AutomatonError::ExecutionFailed(format!("Failed to spawn: {e}")))?;

        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    serde_json::from_str(&stdout)
                        .map_err(|e| AutomatonError::ExecutionFailed(format!("Invalid JSON output: {e}")))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(AutomatonError::ExecutionFailed(format!(
                        "Exit code {}: {}",
                        output.status.code().unwrap_or(-1),
                        stderr,
                    )))
                }
            }
            Ok(Err(e)) => Err(AutomatonError::ExecutionFailed(format!("Process error: {e}"))),
            Err(_) => Err(AutomatonError::Timeout(timeout_ms)),
        }
    }

    /// Run a module with retry support.
    pub async fn run_with_retry(
        &self,
        binary_path: &std::path::Path,
        input: &serde_json::Value,
        retry: &RetryConfig,
        timeout_ms: u64,
    ) -> Result<serde_json::Value> {
        let mut last_error = String::new();
        let mut delay = retry.delay_ms;

        for attempt in 1..=retry.max_attempts {
            tracing::info!(attempt, "Running module attempt");

            match self.run_binary(binary_path, input, timeout_ms).await {
                Ok(output) => return Ok(output),
                Err(e) => {
                    last_error = e.to_string();
                    tracing::warn!(attempt, error = %last_error, "Attempt failed");
                    if attempt < retry.max_attempts {
                        // Sleep with current delay (first attempt uses retry.delay_ms)
                        if delay > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                        }
                        // Calculate next backoff after sleeping
                        delay = match retry.backoff {
                            BackoffKind::Fixed => retry.delay_ms,
                            BackoffKind::Linear => retry.delay_ms * (attempt as u64 + 1),
                            BackoffKind::Exponential => retry.delay_ms * (1u64 << attempt),
                        };
                    }
                }
            }
        }

        Err(AutomatonError::ExecutionFailed(format!(
            "All {} attempts failed. Last error: {last_error}",
            retry.max_attempts
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config_defaults() {
        let config = RuntimeConfig::default();
        assert_eq!(config.max_concurrency, 4);
        assert_eq!(config.default_timeout_ms, 30_000);
    }

    #[test]
    fn test_runtime_config_custom() {
        let config = RuntimeConfig {
            max_concurrency: 8,
            default_timeout_ms: 60_000,
            work_dir: "./custom_work".into(),
            temp_dir: "./custom_tmp".into(),
        };
        assert_eq!(config.max_concurrency, 8);
        assert_eq!(config.default_timeout_ms, 60_000);
    }
}
