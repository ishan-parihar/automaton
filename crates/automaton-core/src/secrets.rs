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
            Self {
                name: "postgresql".into(),
                schema: serde_json::json!({"type":"object"}),
            },
            Self {
                name: "slack".into(),
                schema: serde_json::json!({"type":"object"}),
            },
            Self {
                name: "github".into(),
                schema: serde_json::json!({"type":"object"}),
            },
            Self {
                name: "openai".into(),
                schema: serde_json::json!({"type":"object"}),
            },
            Self {
                name: "anthropic".into(),
                schema: serde_json::json!({"type":"object"}),
            },
            Self {
                name: "http".into(),
                schema: serde_json::json!({"type":"object"}),
            },
            Self {
                name: "aws".into(),
                schema: serde_json::json!({"type":"object"}),
            },
        ]
    }
}

/// AES-256-GCM encryption/decryption for secret values.
/// Master key is read from AUTOMATON_MASTER_KEY environment variable.
pub struct SecretKeeper {
    key: [u8; 32],
}

impl SecretKeeper {
    /// Create a new SecretKeeper, reading the master key from AUTOMATON_MASTER_KEY.
    /// If the env var is not set, a deterministic key is derived from "automaton-default-key".
    pub fn from_env() -> Self {
        let key_hex = std::env::var("AUTOMATON_MASTER_KEY").unwrap_or_default();
        let key = if key_hex.len() == 64 {
            let mut k = [0u8; 32];
            if hex::decode_to_slice(key_hex.as_bytes(), &mut k).is_err() {
                // Fallback on decode failure
                use sha2::Digest;
                let hash = sha2::Sha256::digest(b"automaton-fallback-key");
                k.copy_from_slice(&hash);
            }
            k
        } else {
            let mut k = [0u8; 32];
            use sha2::Digest;
            let hash = sha2::Sha256::digest(b"automaton-fallback-key");
            k.copy_from_slice(&hash);
            k
        };
        Self { key }
    }

    /// Create a SecretKeeper with a specific key (for testing)
    pub fn with_key(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Encrypt a plaintext value.
    /// Returns base64(nonce || ciphertext).
    pub fn encrypt(&self, plaintext: &str) -> String {
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        let cipher = Aes256Gcm::new_from_slice(&self.key).expect("valid key");
        let mut nonce = [0u8; 12];
        use rand::RngCore;
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
            .expect("encryption failed");
        let mut combined = nonce.to_vec();
        combined.extend_from_slice(&ciphertext);
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &combined)
    }

    /// Decrypt a value previously encrypted with `encrypt`.
    /// Input: base64(nonce || ciphertext).
    pub fn decrypt(&self, encrypted: &str) -> Option<String> {
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        let combined = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            encrypted.as_bytes(),
        )
        .ok()?;
        if combined.len() < 12 {
            return None;
        }
        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&self.key).ok()?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .ok()?;
        String::from_utf8(plaintext).ok()
    }
}
