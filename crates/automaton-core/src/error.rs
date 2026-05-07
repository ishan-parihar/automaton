use thiserror::Error;

pub type Result<T> = std::result::Result<T, AutomatonError>;

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for AutomatonError {
    fn from(e: rusqlite::Error) -> Self {
        AutomatonError::Database(e.to_string())
    }
}

#[derive(Error, Debug)]
pub enum AutomatonError {
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    #[error("Module already exists: {0}")]
    ModuleAlreadyExists(String),

    #[error("Build failed: {0}")]
    BuildFailed(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("DAG cycle detected")]
    CyclicDependency,

    #[error("Resolution failed for dependency: {0}")]
    DependencyResolution(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("{0}")]
    Other(String),
}
