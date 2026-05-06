use serde::{Deserialize, Serialize};

/// A stored secret/variable — encrypted at rest
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Variable {
    /// e.g. "slack/api_token" — can be referenced as $var:slack/api_token
    pub path: String,
    /// Encrypted value (the runtime decrypts before injection)
    pub encrypted_value: String,
    /// Whether this is a secret (hidden from UI/logs) or plain config
    pub is_secret: bool,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A typed external resource connection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    /// e.g. "slack/production" — referenced as $res:slack/production
    pub path: String,
    /// e.g. "postgresql", "slack", "github"
    pub resource_type: String,
    /// Connection config
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Known resource types with their schemas
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceType {
    pub name: String,
    pub schema: serde_json::Value,
}

impl ResourceType {
    pub fn builtin_types() -> Vec<Self> {
        vec![
            Self { name: "postgresql".into(), schema: serde_json::json!({"type":"object"}) },
            Self { name: "slack".into(), schema: serde_json::json!({"type":"object"}) },
            Self { name: "github".into(), schema: serde_json::json!({"type":"object"}) },
            Self { name: "openai".into(), schema: serde_json::json!({"type":"object"}) },
            Self { name: "anthropic".into(), schema: serde_json::json!({"type":"object"}) },
            Self { name: "http".into(), schema: serde_json::json!({"type":"object"}) },
            Self { name: "aws".into(), schema: serde_json::json!({"type":"object"}) },
        ]
    }
}
