//! Cron scheduler for Automaton.
//! Evaluates cron expressions and triggers execution.

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
