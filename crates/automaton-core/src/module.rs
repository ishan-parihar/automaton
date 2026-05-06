use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Content hash — SHA-256 of source + metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ContentHash(pub String);

impl ContentHash {
    pub fn compute(bytes: &[u8]) -> Self {
        let hash = Sha256::digest(bytes);
        ContentHash(format!("{:x}", hash))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A dependency reference by name
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DepRef {
    pub name: String,
    pub version_req: Option<String>,
}

impl DepRef {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version_req: None,
        }
    }

    pub fn with_version(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version_req: Some(version.to_string()),
        }
    }
}

/// Retry configuration for a module
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub delay_ms: u64,
    pub backoff: BackoffKind,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_ms: 1000,
            backoff: BackoffKind::Exponential,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackoffKind {
    Fixed,
    Linear,
    Exponential,
}

/// Retry state for a running module
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryState {
    pub attempt: u32,
    pub last_error: String,
    pub next_delay_ms: u64,
}

/// Module identity
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ModuleId {
    /// Human-readable path, e.g. "github.issue_triage"
    pub path: String,
    /// Semantic version
    pub version: semver::Version,
    /// Content hash of the source + metadata
    pub hash: ContentHash,
    /// Time of creation
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Permission descriptor for access control
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Permission {
    pub resource: String,
    pub action: String,
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.resource, self.action)
    }
}
