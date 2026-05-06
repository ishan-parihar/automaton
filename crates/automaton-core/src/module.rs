use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Content hash — SHA-256 of source + metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ContentHash(pub String);

impl ContentHash {
    pub fn compute(bytes: &[u8]) -> Self {
        ContentHash(format!("{:x}", Sha256::digest(bytes)))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

/// A dependency reference by name and optional version constraint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DepRef {
    pub name: String,
    pub version_req: Option<String>,
}

impl DepRef {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), version_req: None }
    }
}

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub delay_ms: u64,
    pub backoff: BackoffKind,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self { max_attempts: 3, delay_ms: 1000, backoff: BackoffKind::Exponential }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackoffKind { Fixed, Linear, Exponential }

/// Module identity — content-addressed with version and hash
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ModuleId {
    pub path: String,
    pub version: semver::Version,
    pub hash: ContentHash,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Execution state for a running module
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionState {
    Pending,
    Running,
    Completed(serde_json::Value),
    Failed(String),
    Skipped(String),
    Retrying(u32),
}

/// Language runtime for scripts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    Go,
    Bash,
    Sql,
}
