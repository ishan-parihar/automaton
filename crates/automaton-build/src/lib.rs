//! Compilation pipeline for Automaton.
//! Builds Rust scripts into native binaries with content-addressed caching.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct BuildCache {
    cache_dir: PathBuf,
    debug_dir: PathBuf,
}

impl BuildCache {
    pub fn new(base_dir: &Path) -> Self {
        let cache_dir = base_dir.join("builds");
        let debug_dir = base_dir.join("build-debug");
        std::fs::create_dir_all(&cache_dir).ok();
        std::fs::create_dir_all(&debug_dir).ok();
        Self { cache_dir, debug_dir }
    }

    /// Check if a compiled artifact exists for the given content hash
    pub fn is_cached(&self, hash: &str) -> bool {
        self.cache_dir.join(hash).join("binary").exists()
    }

    /// Get the path to a cached binary
    pub fn cached_binary(&self, hash: &str) -> PathBuf {
        self.cache_dir.join(hash).join("binary")
    }

    /// Build a Rust source file into a native binary
    pub fn build_rust(
        &self,
        name: &str,
        source: &str,
        manifest: &automaton_core::AutomationManifest,
    ) -> Result<(String, PathBuf), String> {
        let hash = self.compute_hash(source, manifest);
        let artifact_dir = self.cache_dir.join(&hash);

        // Check cache first
        if artifact_dir.join("binary").exists() {
            return Ok((hash, artifact_dir.join("binary")));
        }

        // Create a temporary cargo project for this module
        let tmp_dir = self.debug_dir.join(name.replace('.', "_"));
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).map_err(|e| format!("Cleanup: {e}"))?;
        }

        // Build the project
        let binary = self.compile_cargo_project(&tmp_dir, name, source)
            .map_err(|e| format!("Build failed: {e}"))?;

        // Cache the binary
        std::fs::create_dir_all(&artifact_dir).map_err(|e| format!("Cache dir: {e}"))?;
        std::fs::copy(&binary, artifact_dir.join("binary"))
            .map_err(|e| format!("Cache copy: {e}"))?;

        Ok((hash, binary))
    }

    fn compute_hash(&self, source: &str, manifest: &automaton_core::AutomationManifest) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.update(manifest.name.as_bytes());
        hasher.update(manifest.version.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Create and compile a Cargo project for a single module
    fn compile_cargo_project(
        &self,
        project_dir: &Path,
        name: &str,
        source: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let src_dir = project_dir.join("src");
        std::fs::create_dir_all(&src_dir)?;

        // Write Cargo.toml
        let sanitized = name.replace('.', "_");
        let cargo_toml = format!(
            r#"
[package]
name = "{}"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1"
"#, sanitized
        );
        std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;

        // Write main.rs from source (add entry point wrapper)
        std::fs::write(src_dir.join("main.rs"), source)?;

        // Run cargo build
        let output = Command::new("cargo")
            .args(["build", "--release", "--manifest-path"])
            .arg(project_dir.join("Cargo.toml"))
            .args(["--target-dir", &self.debug_dir.join("target").to_string_lossy()])
            .output()
            .map_err(|e| format!("Failed to run cargo: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Cargo build failed:\n{stderr}").into());
        }

        let binary_path = self.debug_dir.join("target").join("release").join(&sanitized);
        Ok(binary_path)
    }
}
