/// Store scan mode: Automatically detect all built paths
///
/// This module implements automatic store path detection:
/// 1. Scans /nix/store for recent changes
/// 2. Compares filesystem state before/after builds
/// 3. Auto-detects newly built paths without explicit configuration
/// 4. Works as fallback when post-build hooks unavailable
/// 5. Supports incremental scanning (only new paths)

use anyhow::Result;
use console::style;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct StoreScanConfig {
    /// Maximum time to look back for new paths (seconds)
    pub lookback_time: u64,
    /// Minimum file modification time to consider "new" (seconds from now)
    pub min_mtime_delta: u64,
}

impl Default for StoreScanConfig {
    fn default() -> Self {
        Self {
            lookback_time: 3600,        // 1 hour
            min_mtime_delta: 60,        // Modified in last 60 seconds
        }
    }
}

#[derive(Clone, Debug)]
pub struct StoreSnapshot {
    pub timestamp: SystemTime,
    pub paths: HashSet<String>,
    pub path_mtimes: std::collections::HashMap<String, SystemTime>,
}

impl StoreSnapshot {
    /// Create a new snapshot of the Nix store
    pub fn new() -> Result<Self> {
        let timestamp = SystemTime::now();
        let (paths, path_mtimes) = scan_store_paths()?;

        Ok(Self {
            timestamp,
            paths,
            path_mtimes,
        })
    }

    /// Find paths that are new since another snapshot
    pub fn new_paths_since(&self, previous: &StoreSnapshot) -> Vec<String> {
        self.paths
            .iter()
            .filter(|p| !previous.paths.contains(*p))
            .cloned()
            .collect()
    }

    /// Find recently modified paths
    pub fn recently_modified(&self, config: &StoreScanConfig) -> Result<Vec<String>> {
        let cutoff = SystemTime::now() - std::time::Duration::from_secs(config.min_mtime_delta);

        let recent: Vec<String> = self
            .path_mtimes
            .iter()
            .filter(|(_, mtime)| **mtime > cutoff)
            .map(|(path, _)| path.clone())
            .collect();

        Ok(recent)
    }
}

/// Scan the entire /nix/store for all paths
fn scan_store_paths() -> Result<(HashSet<String>, std::collections::HashMap<String, SystemTime>)> {
    let store_path = Path::new("/nix/store");
    let mut paths = HashSet::new();
    let mut mtimes = std::collections::HashMap::new();

    if !store_path.exists() {
        return Err(anyhow::anyhow!(
            "Nix store not found at {}. Is Nix installed?",
            store_path.display()
        ));
    }

    // Scan entries in /nix/store
    for entry in fs::read_dir(store_path)? {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                if let Some(file_name) = path.file_name() {
                    if let Some(name_str) = file_name.to_str() {
                        let full_path = format!("/nix/store/{}", name_str);
                        paths.insert(full_path.clone());

                        // Record modification time
                        if let Ok(metadata) = fs::metadata(&path) {
                            if let Ok(mtime) = metadata.modified() {
                                mtimes.insert(full_path, mtime);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Error reading store entry: {}", e);
            }
        }
    }

    Ok((paths, mtimes))
}

/// Detect paths built since a previous snapshot
pub async fn detect_new_paths(
    previous_snapshot: &StoreSnapshot,
    config: &StoreScanConfig,
) -> Result<Vec<String>> {
    println!(
        "{}",
        style("=== Store Scan: Detecting Newly Built Paths ===\n")
            .bold()
            .cyan()
    );

    // Create new snapshot
    println!("{} Scanning /nix/store...", style("→").cyan());
    let current_snapshot = StoreSnapshot::new()?;

    // Find new paths
    let new_paths = current_snapshot.new_paths_since(previous_snapshot);

    println!(
        "{} Found {} new paths",
        style("✓").green(),
        style(new_paths.len()).bold()
    );

    // Filter by recent modification time
    println!("{} Filtering for recently built paths...", style("→").cyan());
    let recent = current_snapshot.recently_modified(config)?;

    println!(
        "{} {} paths built in last {} seconds",
        style("✓").green(),
        style(recent.len()).bold(),
        config.min_mtime_delta
    );

    // Return intersection of new AND recent
    let final_paths: Vec<String> = new_paths
        .iter()
        .filter(|p| recent.contains(p))
        .cloned()
        .collect();

    Ok(final_paths)
}

/// Perform full store scan without snapshot comparison
pub async fn full_store_scan(config: &StoreScanConfig) -> Result<Vec<String>> {
    println!(
        "{}",
        style("=== Full Store Scan ===\n").bold().cyan()
    );

    println!("{} Scanning /nix/store for recent builds...", style("→").cyan());

    let snapshot = StoreSnapshot::new()?;
    let recent_paths = snapshot.recently_modified(config)?;

    println!(
        "{} Found {} recently built paths",
        style("✓").green(),
        style(recent_paths.len()).bold()
    );

    // Print found paths
    if !recent_paths.is_empty() {
        println!("\n{} Paths to upload:", style("→").cyan());
        for (i, path) in recent_paths.iter().enumerate() {
            println!("  {}. {}", i + 1, style(path).dim());
        }
    }

    Ok(recent_paths)
}

/// Compare two snapshots and report differences
pub fn compare_snapshots(before: &StoreSnapshot, after: &StoreSnapshot) -> ScanDifference {
    let new_paths: HashSet<_> = after
        .paths
        .iter()
        .filter(|p| !before.paths.contains(*p))
        .cloned()
        .collect();

    let removed_paths: HashSet<_> = before
        .paths
        .iter()
        .filter(|p| !after.paths.contains(*p))
        .cloned()
        .collect();

    let modified_paths: HashSet<_> = before
        .path_mtimes
        .iter()
        .filter_map(|(path, before_mtime)| {
            after
                .path_mtimes
                .get(path)
                .and_then(|after_mtime| {
                    if after_mtime > before_mtime {
                        Some(path.clone())
                    } else {
                        None
                    }
                })
        })
        .collect();

    ScanDifference {
        new_paths,
        removed_paths,
        modified_paths,
        total_paths: after.paths.len(),
    }
}

#[derive(Debug, Clone)]
pub struct ScanDifference {
    pub new_paths: HashSet<String>,
    pub removed_paths: HashSet<String>,
    pub modified_paths: HashSet<String>,
    pub total_paths: usize,
}

impl ScanDifference {
    pub fn print_summary(&self) {
        println!(
            "\n{} Store scan summary:",
            style("→").cyan()
        );
        println!(
            "  {} Total paths: {}",
            style("·").dim(),
            style(self.total_paths).bold()
        );
        println!(
            "  {} New paths: {}",
            style("·").dim(),
            style(self.new_paths.len()).green()
        );
        println!(
            "  {} Modified: {}",
            style("·").dim(),
            style(self.modified_paths.len()).yellow()
        );
        println!(
            "  {} Removed: {}",
            style("·").dim(),
            style(self.removed_paths.len()).red()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = StoreScanConfig::default();
        assert_eq!(config.lookback_time, 3600);
        assert_eq!(config.min_mtime_delta, 60);
    }

    #[test]
    fn test_snapshot_creation() {
        // Note: This test requires /nix/store to exist
        if Path::new("/nix/store").exists() {
            let snapshot = StoreSnapshot::new();
            assert!(snapshot.is_ok());
        }
    }
}
