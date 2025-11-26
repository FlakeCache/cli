/// Daemon mode: Post-build hooks for transparent, background uploads
///
/// This module implements a daemon that:
/// 1. Watches for new store paths after builds
/// 2. Automatically uploads them to FlakeCache
/// 3. Runs in the background without blocking builds
/// 4. Handles failures gracefully (non-blocking)

use anyhow::Result;
use console::style;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::time::sleep;

#[derive(Clone, Debug)]
pub struct DaemonConfig {
    pub cache_name: String,
    pub api_url: String,
    pub token: String,
    pub watch_interval: Duration,
    pub log_dir: PathBuf,
}

impl DaemonConfig {
    pub fn new(cache_name: String, api_url: String, token: String) -> Self {
        let log_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("flakecache")
            .join("daemon");

        Self {
            cache_name,
            api_url,
            token,
            watch_interval: Duration::from_secs(5),
            log_dir,
        }
    }

    pub fn log_file(&self) -> PathBuf {
        self.log_dir.join("daemon.log")
    }

    pub fn state_file(&self) -> PathBuf {
        self.log_dir.join("uploaded_paths.txt")
    }
}

/// Start daemon mode: watches for new store paths and uploads them
pub async fn start_daemon(config: DaemonConfig) -> Result<()> {
    println!(
        "{}",
        style("=== FlakeCache Daemon Mode (Background Upload Service) ===\n")
            .bold()
            .cyan()
    );

    // Create log directory
    fs::create_dir_all(&config.log_dir)?;

    println!(
        "{} Daemon started for cache: {}",
        style("✓").green(),
        style(&config.cache_name).bold()
    );
    println!(
        "{} Watch interval: {:?}",
        style("✓").green(),
        config.watch_interval
    );
    println!(
        "{} Log file: {}",
        style("✓").green(),
        config.log_file().display()
    );

    // Load previously uploaded paths
    let mut uploaded_paths = load_uploaded_paths(&config.state_file())?;

    println!(
        "{} Loaded {} previously uploaded paths",
        style("✓").green(),
        uploaded_paths.len()
    );

    // Main daemon loop
    println!("{} Watching for new store paths...\n", style("→").cyan());

    loop {
        // Get current store paths from nix
        match get_store_paths().await {
            Ok(current_paths) => {
                // Find new paths not yet uploaded
                let new_paths: Vec<String> = current_paths
                    .iter()
                    .filter(|p| !uploaded_paths.contains(*p))
                    .cloned()
                    .collect();

                if !new_paths.is_empty() {
                    println!(
                        "{} Found {} new store paths to upload",
                        style("→").cyan(),
                        new_paths.len()
                    );

                    // Upload new paths in background
                    for path in &new_paths {
                        match upload_path(&config, path).await {
                            Ok(_) => {
                                uploaded_paths.insert(path.clone());
                                save_uploaded_paths(&config.state_file(), &uploaded_paths)?;
                                log_message(
                                    &config.log_file(),
                                    &format!("✓ Uploaded: {}", path),
                                )?;
                                println!("{} Uploaded: {}", style("✓").green(), path);
                            }
                            Err(e) => {
                                log_message(
                                    &config.log_file(),
                                    &format!("✗ Failed to upload {}: {}", path, e),
                                )?;
                                println!("{} Failed: {} ({})", style("⚠").yellow(), path, e);
                                // Continue - don't block daemon on single upload failure
                            }
                        }
                    }
                } else {
                    println!("{} No new paths ({})", style("·").dim(), now_timestamp());
                }
            }
            Err(e) => {
                log_message(&config.log_file(), &format!("Error fetching paths: {}", e))?;
                println!("{} Error: {}", style("✗").red(), e);
            }
        }

        // Wait before next check
        sleep(config.watch_interval).await;
    }
}

/// Get all store paths from the Nix store
async fn get_store_paths() -> Result<Vec<String>> {
    let output = tokio::process::Command::new("nix")
        .args(&["store", "ls", "/nix/store"])
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("Failed to query Nix store"));
    }

    let paths: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| format!("/nix/store/{}", line.trim()))
        .collect();

    Ok(paths)
}

/// Upload a single store path
async fn upload_path(config: &DaemonConfig, path: &str) -> Result<()> {
    use crate::upload;

    println!("{} Uploading {} to {}", style("→").cyan(), path, config.cache_name);

    // Use the reusable upload function from the upload module
    upload::upload_single_store_path(path, &config.cache_name, &config.api_url, &config.token).await
}

/// Load previously uploaded paths from state file
fn load_uploaded_paths(state_file: &Path) -> Result<std::collections::HashSet<String>> {
    if !state_file.exists() {
        return Ok(std::collections::HashSet::new());
    }

    let content = fs::read_to_string(state_file)?;
    let paths = content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(paths)
}

/// Save uploaded paths to state file
fn save_uploaded_paths(
    state_file: &Path,
    paths: &std::collections::HashSet<String>,
) -> Result<()> {
    let content = paths
        .iter()
        .map(|p| format!("{}\n", p))
        .collect::<String>();

    fs::write(state_file, content)?;
    Ok(())
}

/// Log a message to daemon log file
fn log_message(log_file: &Path, message: &str) -> Result<()> {
    let timestamp = now_timestamp();
    let log_entry = format!("[{}] {}\n", timestamp, message);

    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;

    file.write_all(log_entry.as_bytes())?;
    Ok(())
}

/// Get current timestamp for logging
fn now_timestamp() -> String {
    use chrono::Local;
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_config_creation() {
        let config = DaemonConfig::new(
            "test-cache".to_string(),
            "https://c.flakecache.com".to_string(),
            "test-token".to_string(),
        );

        assert_eq!(config.cache_name, "test-cache");
        assert_eq!(config.watch_interval, Duration::from_secs(5));
    }
}
