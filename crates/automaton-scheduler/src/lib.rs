//! Cron scheduler for Automaton.
//! Evaluates cron expressions and triggers execution.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use croner::Cron;

pub struct Scheduler;

impl Scheduler {
    /// Validate a cron expression
    pub fn validate(schedule: &str) -> Result<(), String> {
        let mut c = Cron::new(schedule);
        c.with_seconds_optional();
        c.parse()
            .map_err(|e| format!("Invalid cron expression '{schedule}': {e}"))?;
        Ok(())
    }

    /// Check if a cron expression matches the given time
    pub fn matches(schedule: &str, time: &chrono::DateTime<chrono::Utc>) -> Result<bool, String> {
        let mut c = Cron::new(schedule);
        c.with_seconds_optional();
        let cron = c.parse().map_err(|e| format!("Invalid cron: {e}"))?;
        cron.is_time_matching(time)
            .map_err(|e| format!("Cron error: {e}"))
    }

    /// Find the next occurrence after a given time
    pub fn next_occurrence(
        schedule: &str,
        after: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        let mut c = Cron::new(schedule);
        c.with_seconds_optional();
        let cron = c.parse().map_err(|e| format!("Invalid cron: {e}"))?;
        match cron.find_next_occurrence(after, false) {
            Ok(t) => Ok(Some(t)),
            Err(_) => Ok(None),
        }
    }
}

/// A simple cron ticker that tracks the last checked minute
pub struct CronTicker {
    schedule: String,
    last_minute: i64,
}

impl CronTicker {
    pub fn new(schedule: &str) -> Self {
        Self {
            schedule: schedule.to_string(),
            last_minute: chrono::Utc::now().timestamp() / 60,
        }
    }

    /// Returns true if the cron schedule should fire (max once per minute)
    pub fn tick(&mut self) -> bool {
        let now = chrono::Utc::now();
        let current_minute = now.timestamp() / 60;
        if current_minute == self.last_minute {
            return false;
        }
        self.last_minute = current_minute;
        let mut c = Cron::new(&self.schedule);
        c.with_seconds_optional();
        match c.parse() {
            Ok(cron) => cron.is_time_matching(&now).unwrap_or(false),
            Err(_) => false,
        }
    }
}

/// A scheduled trigger definition returned by the trigger provider.
#[derive(Debug, Clone)]
pub struct ScheduledTrigger {
    pub id: String,
    pub target_path: String,
    pub target_is_flow: bool,
    pub schedule: String,
    pub args: Option<serde_json::Value>,
}

/// Trait for providing cron triggers to the scheduler daemon.
#[async_trait::async_trait]
pub trait TriggerProvider: Send + Sync {
    async fn get_cron_triggers(&self) -> Result<Vec<ScheduledTrigger>, String>;
    async fn enqueue_job(&self, kind: &str, target: &str, args: &serde_json::Value) -> Result<i64, String>;
}

/// Daemon that polls for cron triggers and fires them into the job queue.
/// Runs as a background tokio task. Shuts down via `stop()`.
pub struct SchedulerDaemon {
    shutdown: Arc<AtomicBool>,
}

impl SchedulerDaemon {
    /// Start the scheduler daemon. Spawns a tokio task that polls the
    /// TriggerProvider every `poll_interval_ms` and fires matching triggers.
    pub fn start<P>(provider: Arc<P>, poll_interval_ms: u64) -> Self
    where
        P: TriggerProvider + Send + Sync + 'static,
    {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            tracing::info!(
                "Scheduler daemon started (poll interval: {}ms)",
                poll_interval_ms
            );
            let mut tickers: Vec<(String, CronTicker)> = Vec::new();

            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    tracing::info!("Scheduler daemon shutting down");
                    break;
                }

                // Get all enabled cron triggers
                match provider.get_cron_triggers().await {
                    Ok(triggers) => {
                        // Ensure we have a ticker for each trigger
                        for t in &triggers {
                            if !tickers.iter().any(|(id, _)| id == &t.id) {
                                tickers.push((t.id.clone(), CronTicker::new(&t.schedule)));
                            }
                        }

                        // Fire matching triggers
                        for t in &triggers {
                            let should_fire = tickers
                                .iter_mut()
                                .find(|(id, _)| id == &t.id)
                                .map(|(_, ticker)| ticker.tick())
                                .unwrap_or(false);

                            if should_fire {
                                let kind = if t.target_is_flow {
                                    "flow"
                                } else {
                                    "script"
                                };
                                let args = t.args.as_ref().cloned().unwrap_or_default();
                                match provider.enqueue_job(kind, &t.target_path, &args).await {
                                    Ok(job_id) => {
                                        tracing::info!(
                                            trigger = %t.id,
                                            target = %t.target_path,
                                            job_id = job_id,
                                            "Cron trigger fired"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            trigger = %t.id,
                                            error = %e,
                                            "Failed to enqueue job for cron trigger"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to fetch cron triggers: {}", e);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;
            }
        });

        Self { shutdown }
    }

    /// Signal the daemon to shut down gracefully.
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_cron() {
        assert!(Scheduler::validate("0 0 * * *").is_ok());
        assert!(Scheduler::validate("*/5 * * * *").is_ok());
        assert!(Scheduler::validate("0 9-17 * * 1-5").is_ok());
    }

    #[test]
    fn test_validate_invalid_cron() {
        assert!(Scheduler::validate("").is_err());
        assert!(Scheduler::validate("not-a-cron").is_err());
        assert!(Scheduler::validate("* * * * * * * *").is_err());
    }

    #[test]
    fn test_cron_ticker_initialization() {
        let mut ticker = CronTicker::new("0 * * * *");
        // First call should return false (same minute)
        assert!(!ticker.tick());
    }
}
