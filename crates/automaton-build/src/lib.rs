//! Compilation pipeline for Automaton.
//! Builds Rust scripts into native binaries with content-addressed caching.

pub mod templates;

use std::path::{Path, PathBuf};
use std::process::Command;

/// A single structured diagnostic from a cargo build failure.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildDiagnostic {
    /// Severity: "error" or "warning"
    pub severity: String,
    /// Error code like "E0432" or "E0308"
    pub code: Option<String>,
    /// The error message
    pub message: String,
    /// File path (relative to build project)
    pub file: Option<String>,
    /// Line number (1-based)
    pub line: Option<usize>,
    /// Column number (1-based)
    pub column: Option<usize>,
    /// The cited code snippet, if any
    pub snippet: Option<String>,
}

/// Structured build diagnostics result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildDiagnostics {
    pub success: bool,
    pub hash: Option<String>,
    pub diagnostics: Vec<BuildDiagnostic>,
    pub raw_stderr: String,
}

/// Try to find the automaton project root by looking at the current executable path.
fn find_project_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    // Walk up to find Cargo.toml with automaton workspace
    for _ in 0..10 {
        if dir.join("Cargo.toml").exists() {
            // Check if this is the automaton workspace by looking for crates/automaton-sdk
            if dir
                .join("crates")
                .join("automaton-sdk")
                .join("Cargo.toml")
                .exists()
            {
                return Some(dir.to_path_buf());
            }
        }
        dir = dir.parent()?;
    }
    None
}

#[derive(Clone)]
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
        Self {
            cache_dir,
            debug_dir,
        }
    }

    /// Check if a compiled artifact exists for the given content hash
    pub fn is_cached(&self, hash: &str) -> bool {
        self.cache_dir.join(hash).join("binary").exists()
    }

    /// Get the path to a cached binary
    pub fn cached_binary(&self, hash: &str) -> PathBuf {
        self.cache_dir.join(hash).join("binary")
    }

    /// Parse cargo stderr into structured diagnostics.
    pub fn diagnose(raw_stderr: &str) -> Vec<BuildDiagnostic> {
        let mut diagnostics = Vec::new();
        let lines: Vec<&str> = raw_stderr.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Match: error[E0432]: message  or  warning[unused]: message  or  error: message
            let sev_code_msg = if let Some(stripped) = line.strip_prefix("error[") {
                stripped.find(']').map(|end_bracket| {
                    (
                        "error",
                        Some(stripped[..end_bracket].to_string()),
                        stripped[end_bracket + 2..].to_string(),
                    )
                })
            } else if let Some(stripped) = line.strip_prefix("warning[") {
                stripped.find(']').map(|end_bracket| {
                    (
                        "warning",
                        Some(stripped[..end_bracket].to_string()),
                        stripped[end_bracket + 2..].to_string(),
                    )
                })
            } else if line.starts_with("error:") {
                Some(("error", None, line[6..].trim().to_string()))
            } else if line.starts_with("warning:") {
                Some(("warning", None, line[8..].trim().to_string()))
            } else {
                None
            };

            if let Some((severity, code, message)) = sev_code_msg {
                let mut diagnostic = BuildDiagnostic {
                    severity: severity.to_string(),
                    code,
                    message,
                    file: None,
                    line: None,
                    column: None,
                    snippet: None,
                };

                // Next line should be location:   --> src/main.rs:10:5
                if i + 1 < lines.len() {
                    let loc_line = lines[i + 1].trim();
                    if let Some(loc) = loc_line.strip_prefix("-->") {
                        let loc = loc.trim();
                        // Parse file:line:column
                        let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
                        if parts.len() >= 3 {
                            diagnostic.column = parts[0].parse().ok();
                            diagnostic.line = parts[1].parse().ok();
                            // File is everything before the last two colons
                            diagnostic.file = Some(parts[2..].join(":").to_string());
                        }

                        // Skip the location line (the `-->` line), not the separator
                        i += 1;
                        // Collect snippet lines: separator |, numbered source |, annotation |, = help
                        let mut snippet_lines = Vec::new();
                        while i < lines.len() {
                            let snippet_line = lines[i].trim();
                            if snippet_line.starts_with('|')
                                || snippet_line.starts_with('=')
                                || snippet_line.contains("^^^")
                                || snippet_line.contains("~~")
                            {
                                snippet_lines.push(snippet_line.to_string());
                                i += 1;
                            } else if snippet_line.is_empty()
                                || snippet_line.starts_with("error")
                                || snippet_line.starts_with("warning")
                            {
                                break;
                            } else {
                                // Numbered source line like `10 | use foo;` — still a snippet
                                // Check if it contains a pipe
                                if snippet_line.contains(" | ") || snippet_line.contains("|") {
                                    snippet_lines.push(snippet_line.to_string());
                                    i += 1;
                                } else {
                                    break;
                                }
                            }
                        }
                        if !snippet_lines.is_empty() {
                            diagnostic.snippet = Some(snippet_lines.join("\n"));
                        }
                        diagnostics.push(diagnostic);
                        continue; // i already advanced
                    }
                }
                diagnostics.push(diagnostic);
            }
            i += 1;
        }

        diagnostics
    }

    /// Build a Rust source file into a native binary
    pub fn build_rust(
        &self,
        name: &str,
        source: &str,
        manifest: &automaton_core::AutomationManifest,
    ) -> Result<(String, PathBuf), String> {
        self.build_rust_with_deps(name, source, manifest, &[])
    }

    /// Build with extra Cargo dependencies (for template-generated modules)
    pub fn build_rust_with_deps(
        &self,
        name: &str,
        source: &str,
        manifest: &automaton_core::AutomationManifest,
        extra_deps: &[(&str, &str)], // (crate_name, version_spec)
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
        let binary = self
            .compile_cargo_project(&tmp_dir, name, source, Some(extra_deps))
            .map_err(|e| format!("Build failed: {e}"))?;

        // Cache the binary (content-addressed by hash)
        std::fs::create_dir_all(&artifact_dir).map_err(|e| format!("Cache dir: {e}"))?;
        std::fs::copy(&binary, artifact_dir.join("binary"))
            .map_err(|e| format!("Cache copy: {e}"))?;

        // Also place at predictable path for the CLI run command
        let predictable_path = self.cache_dir.join(name.replace('.', "_"));
        std::fs::copy(&binary, &predictable_path).ok();

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
        extra_deps: Option<&[(&str, &str)]>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let src_dir = project_dir.join("src");
        std::fs::create_dir_all(&src_dir)?;

        // Write Cargo.toml
        let sanitized = name.replace('.', "_");

        // Build extra dependency lines
        let mut extra_dep_lines = String::new();
        if let Some(deps) = extra_deps {
            for (dep_name, dep_ver) in deps {
                extra_dep_lines.push_str(&format!("{} = {}\n", dep_name, dep_ver));
            }
        }

        // If source uses automaton-sdk, add it as a path dependency
        let sdk_dep = if source.contains("automaton_sdk") || source.contains("#[automaton]") {
            if let Some(project_root) = find_project_root() {
                let sdk_path = project_root.join("crates").join("automaton-sdk");
                if sdk_path.exists() {
                    format!(
                        r#"
automaton-sdk = {{ path = "{}" }}
schemars = "0.8"
uuid = {{ version = "1", features = ["v4"] }}
"#,
                        sdk_path.to_string_lossy()
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

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
"#,
            sanitized
        );
        let cargo_toml = cargo_toml + &extra_dep_lines + &sdk_dep;
        std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;

        // Write main.rs from source (add entry point wrapper)
        std::fs::write(src_dir.join("main.rs"), source)?;

        // Run cargo build
        let output = Command::new("cargo")
            .args(["build", "--release", "--manifest-path"])
            .arg(project_dir.join("Cargo.toml"))
            .args([
                "--target-dir",
                &self.debug_dir.join("target").to_string_lossy(),
            ])
            .output()
            .map_err(|e| format!("Failed to run cargo: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Cargo build failed:\n{stderr}").into());
        }

        let binary_path = self
            .debug_dir
            .join("target")
            .join("release")
            .join(&sanitized);
        Ok(binary_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::all_templates;

    #[test]
    fn test_diagnose_simple_error() {
        let stderr = "error[E0432]: unresolved import `foo`
  --> src/main.rs:10:5
   |
10 | use foo;
   |     ^^^ no `foo` in the root
";
        let diags = BuildCache::diagnose(stderr);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, "error");
        assert_eq!(diags[0].code.as_deref(), Some("E0432"));
    }

    #[test]
    fn test_diagnose_without_code() {
        let stderr = "error: aborting due to previous error\n";
        let diags = BuildCache::diagnose(stderr);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, "error");
        assert!(diags[0].code.is_none());
    }

    #[test]
    fn test_diagnose_warning() {
        let stderr = "warning[unused_variables]: unused variable: `x`\n  --> src/main.rs:5:9\n   |\n5  |     let x = 5;\n   |         ^ help: prefix with underscore\n";
        let diags = BuildCache::diagnose(stderr);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, "warning");
        assert!(diags[0]
            .file
            .as_deref()
            .unwrap_or("")
            .contains("src/main.rs"));
    }

    #[test]
    fn test_diagnose_empty() {
        let diags = BuildCache::diagnose("");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_all_templates_exist() {
        let templates = all_templates();
        assert!(!templates.is_empty());
        let names: Vec<&str> = templates.iter().map(|t| t.name).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"http-fetch"));
        assert!(names.contains(&"data-transform"));
    }
}
