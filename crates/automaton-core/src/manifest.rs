use crate::module::{ContentHash, DepRef, RetryConfig};
use serde::{Deserialize, Serialize};

/// The serializable manifest for an automation module.
/// Mirrors Windmill's script.yaml pattern.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomationManifest {
    /// Unique path-style name, e.g. "github.issue_triage"
    pub name: String,

    /// Semantic version
    pub version: String,

    /// Which function to call (matches the #[automation] fn name)
    pub entry: String,

    /// Optional summary / description
    pub summary: Option<String>,
    pub description: Option<String>,

    /// JSON Schema for inputs (auto-generated if "auto")
    pub inputs_schema: SchemaMode,

    /// JSON Schema for outputs (auto-generated if "auto")
    pub outputs_schema: SchemaMode,

    /// Required permissions
    pub permissions: Vec<String>,

    /// Bound resources
    pub resources: Vec<String>,

    /// Module dependencies
    pub depends_on: Vec<DepRef>,

    /// Retry policy
    pub retry: Option<RetryConfig>,

    /// Timeout in milliseconds
    pub timeout_ms: u64,

    /// Tags for categorization
    pub tags: Vec<String>,

    /// Human-in-the-loop: require approval before execution
    pub require_approval: bool,
}

impl Default for AutomationManifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: "0.1.0".to_string(),
            entry: "main".to_string(),
            summary: None,
            description: None,
            inputs_schema: SchemaMode::Auto,
            outputs_schema: SchemaMode::Auto,
            permissions: vec![],
            resources: vec![],
            depends_on: vec![],
            retry: Some(RetryConfig::default()),
            timeout_ms: 30_000,
            tags: vec![],
            require_approval: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SchemaMode {
    Auto,
    Inline(serde_json::Value),
}

impl Default for SchemaMode {
    fn default() -> Self {
        SchemaMode::Auto
    }
}

/// A full automation module — manifest + source + hash
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomationModule {
    pub manifest: AutomationManifest,
    pub source: String,
    pub hash: ContentHash,
    pub built: bool,
}
