use serde::{Deserialize, Serialize};

/// A trigger that invokes a script or flow
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Trigger {
    pub id: String,
    /// What to trigger — script path or flow path
    pub target_path: String,
    pub target_is_flow: bool,
    pub trigger_type: TriggerType,
    pub config: TriggerConfig,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Kind of trigger
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TriggerType {
    Cron,
    Webhook,
    Event,
}

/// Configuration for each trigger type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
pub struct TriggerConfig {
    /// Cron expression (for Cron triggers)
    pub schedule: Option<String>,
    /// Timezone for cron
    pub timezone: Option<String>,
    /// Skip condition (JS expression evaluated before run)
    pub skip_if: Option<String>,
    /// Webhook secret (for Webhook triggers)
    pub webhook_secret: Option<String>,
    /// Event source (for Event triggers)
    pub event_source: Option<String>,
    /// Arguments to pass to the target script/flow
    pub args: Option<serde_json::Value>,
}

