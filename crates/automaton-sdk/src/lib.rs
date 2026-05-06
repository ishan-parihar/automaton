/// Automaton SDK — authoring macros and runtime helpers.
///
/// The `#[automaton]` macro marks an async `main` function as
/// an automation module entrypoint.
///
/// # Example
/// ```ignore
/// use automaton_sdk::prelude::*;
///
/// #[automaton::main]
/// async fn main(ctx: Context, input: MyInput) -> anyhow::Result<MyOutput> {
///     // ...
/// }
/// ```
pub use automaton_core::{AutomationManifest, ContentHash, ModuleId};

/// Re-export the derive proc macro
pub use automaton_sdk_derive::automation;

/// Standard prelude for automation authors
pub mod prelude {
    pub use crate::automation;
    pub use automaton_core::{
        AutomationManifest, BackoffKind, ContentHash, DepRef, ModuleId, RetryConfig,
    };
    pub use schemars::JsonSchema;
    pub use serde::{Deserialize, Serialize};
}

/// Context injected into every automation execution.
/// Provides access to resources, secrets, and runtime info.
#[derive(Clone, Debug)]
pub struct Context {
    /// Unique run ID
    pub run_id: String,
    /// Module name
    pub module_name: String,
    /// Execution attempt (1-based, incremented on retries)
    pub attempt: u32,
}

impl Context {
    pub fn new(module_name: &str) -> Self {
        Self {
            run_id: uuid::Uuid::new_v4().to_string(),
            module_name: module_name.to_string(),
            attempt: 1,
        }
    }
}
