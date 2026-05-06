use serde::{Deserialize, Serialize};

use crate::module::{ContentHash, DepRef, RetryConfig};

/// The serializable manifest for an automation module
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomationManifest {
    pub name: String,
    pub version: String,
    pub entry: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub inputs_schema: SchemaMode,
    pub outputs_schema: SchemaMode,
    pub permissions: Vec<String>,
    pub resources: Vec<String>,
    pub depends_on: Vec<DepRef>,
    pub retry: Option<RetryConfig>,
    pub timeout_ms: u64,
    pub tags: Vec<String>,
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
#[derive(Default)]
pub enum SchemaMode {
    #[default]
    Auto,
    Inline(serde_json::Value),
}

/// A full automation module — manifest + source + hash
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomationModule {
    pub manifest: AutomationManifest,
    pub source: String,
    pub hash: ContentHash,
    pub built: bool,
}
