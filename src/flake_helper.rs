use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Represents a Nix store path result from a build
#[derive(Debug, Serialize, Deserialize)]
pub struct BuildResult {
    pub outputs: std::collections::HashMap<String, String>,
}

/// Build a flake output and return the resulting store paths
///
/// Example inputs:
/// - ".#hello" -> builds hello from current directory flake
/// - "github:nixos/nixpkgs#hello" -> builds hello from nixpkgs
/// - ".#packages.x86_64-linux.myapp" -> builds specific output
#[allow(clippy::unused_async)] // Async signature for API consistency
pub async fn build_flake_output(flake_ref: &str) -> Result<Vec<String>> {
    println!("🔨 Building {flake_ref}...");

    // Run `nix build --json <flake-ref>`
    let output = Command::new("nix")
        .args(["build", "--json", "--no-link", flake_ref])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Failed to build {flake_ref}: {stderr}"));
    }

    // Parse JSON output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let results: Vec<BuildResult> = serde_json::from_str(&stdout)?;

    // Extract all store paths from all outputs
    let mut store_paths = Vec::new();
    for result in results {
        for (_, path) in result.outputs {
            store_paths.push(path);
        }
    }

    if store_paths.is_empty() {
        return Err(anyhow::anyhow!("No store paths produced by {flake_ref}"));
    }

    println!("✅ Built {} store path(s)", store_paths.len());
    for path in &store_paths {
        println!("   {path}");
    }

    Ok(store_paths)
}

/// Get all dependencies (runtime closure) for a store path
#[allow(clippy::unused_async)] // Async signature for API consistency
pub async fn get_store_path_closure(store_path: &str) -> Result<Vec<String>> {
    let output = Command::new("nix-store")
        .args(["--query", "--requisites", store_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Failed to query requisites: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<String> = stdout.lines().map(ToString::to_string).collect();

    Ok(paths)
}

/// Detect if input is a flake reference (contains # or is a URL)
pub fn is_flake_reference(input: &str) -> bool {
    input.contains('#')
        || input.starts_with("github:")
        || input.starts_with("gitlab:")
        || input.starts_with("git+")
        || input.starts_with("path:")
        || input.starts_with("tarball:")
}

/// Parse store path specification - could be:
/// 1. A flake reference like ".#hello"
/// 2. A direct store path like "/nix/store/..."
/// 3. A package name that should be resolved
pub async fn resolve_to_store_paths(input: &str) -> Result<Vec<String>> {
    // If it's already a store path, return as-is
    if input.starts_with("/nix/store/") {
        return Ok(vec![input.to_string()]);
    }

    // If it's a flake reference, build it
    if is_flake_reference(input) {
        return build_flake_output(input).await;
    }

    // Otherwise, try to resolve as a package
    // This handles cases like "hello" -> "nixpkgs#hello"
    let flake_ref = if input.contains('#') {
        input.to_string()
    } else {
        format!("nixpkgs#{input}")
    };

    build_flake_output(&flake_ref).await
}

/// Get all inputs (dependencies) for the current flake
#[allow(dead_code)]
pub async fn get_flake_inputs() -> Result<Vec<String>> {
    println!("🔍 Analyzing flake inputs...");

    // Run `nix flake metadata --json`
    let output = Command::new("nix")
        .args(["flake", "metadata", "--json"])
        .output()?;

    if !output.status.success() {
        // If no flake.nix, return empty
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let metadata: serde_json::Value = serde_json::from_str(&stdout)?;

    let mut input_paths = Vec::new();

    // Get locked inputs
    if let Some(locks) = metadata.get("locks").and_then(|l| l.get("nodes")) {
        if let Some(obj) = locks.as_object() {
            for (name, node) in obj {
                if name == "root" {
                    continue;
                }

                // Try to get the store path for this input
                if let Some(locked) = node.get("locked") {
                    if let Some(path) = locked.get("path").and_then(|p| p.as_str()) {
                        if path.starts_with("/nix/store/") {
                            input_paths.push(path.to_string());
                        }
                    }

                    // For flake inputs, we can also try to evaluate them
                    if let Some(original) = node.get("original") {
                        if let Some(ref_str) = format_flake_ref(original) {
                            // Build the input to get its path
                            if let Ok(paths) = build_flake_output(&ref_str).await {
                                input_paths.extend(paths);
                            }
                        }
                    }
                }
            }
        }
    }

    if !input_paths.is_empty() {
        println!("✅ Found {} input path(s)", input_paths.len());
    }

    Ok(input_paths)
}

/// Format a flake reference from metadata JSON
#[allow(dead_code)]
fn format_flake_ref(original: &serde_json::Value) -> Option<String> {
    if let Some(r#type) = original.get("type").and_then(|t| t.as_str()) {
        match r#type {
            "github" => {
                let owner = original.get("owner")?.as_str()?;
                let repo = original.get("repo")?.as_str()?;
                Some(format!("github:{owner}/{repo}"))
            }
            "gitlab" => {
                let owner = original.get("owner")?.as_str()?;
                let repo = original.get("repo")?.as_str()?;
                Some(format!("gitlab:{owner}/{repo}"))
            }
            "path" => {
                let path = original.get("path")?.as_str()?;
                Some(format!("path:{path}"))
            }
            _ => None,
        }
    } else {
        None
    }
}
