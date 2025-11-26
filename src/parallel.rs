/// Parallel uploads: Upload multiple store paths concurrently
///
/// This module implements concurrent, parallelized uploads:
/// 1. Uploads multiple store paths in parallel (not sequential)
/// 2. Configurable concurrency level (default: 4 concurrent uploads)
/// 3. Better utilization of network bandwidth
/// 4. Faster total upload time for large artifacts
/// 5. Graceful error handling per path

use anyhow::Result;
use console::style;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone, Debug)]
pub struct ParallelUploadConfig {
    /// Maximum concurrent uploads (default: 4)
    pub concurrency: usize,
    /// Timeout per upload in seconds (default: 300)
    pub timeout_secs: u64,
}

impl Default for ParallelUploadConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            timeout_secs: 300,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UploadTask {
    pub store_path: String,
    pub cache_name: String,
    pub api_url: String,
    pub token: String,
}

#[derive(Clone, Debug)]
pub struct UploadResult {
    pub store_path: String,
    pub success: bool,
    pub error: Option<String>,
    pub duration_secs: u64,
}

/// Execute parallel uploads of store paths
pub async fn upload_parallel(
    tasks: Vec<UploadTask>,
    config: ParallelUploadConfig,
) -> Result<Vec<UploadResult>> {
    if tasks.is_empty() {
        println!("{} No store paths to upload", style("·").dim());
        return Ok(Vec::new());
    }

    println!(
        "{}",
        style(format!(
            "=== Uploading {} paths in parallel (concurrency: {}) ===\n",
            tasks.len(),
            config.concurrency
        ))
        .bold()
        .cyan()
    );

    let semaphore = Arc::new(Semaphore::new(config.concurrency));

    // Create parallel upload streams
    let upload_futures = tasks.into_iter().map(|task| {
        let semaphore = Arc::clone(&semaphore);
        let config = config.clone();

        async move {
            // Acquire permit (limits concurrency)
            let _permit = semaphore.acquire().await.unwrap();

            let start_time = std::time::Instant::now();

            // Perform upload with timeout
            let result = match tokio::time::timeout(
                std::time::Duration::from_secs(config.timeout_secs),
                upload_single(&task),
            )
            .await
            {
                Ok(Ok(())) => UploadResult {
                    store_path: task.store_path.clone(),
                    success: true,
                    error: None,
                    duration_secs: start_time.elapsed().as_secs(),
                },
                Ok(Err(e)) => UploadResult {
                    store_path: task.store_path.clone(),
                    success: false,
                    error: Some(e.to_string()),
                    duration_secs: start_time.elapsed().as_secs(),
                },
                Err(_) => UploadResult {
                    store_path: task.store_path.clone(),
                    success: false,
                    error: Some(format!(
                        "Upload timeout (>{} secs)",
                        config.timeout_secs
                    )),
                    duration_secs: start_time.elapsed().as_secs(),
                },
            };

            // Print result immediately
            if result.success {
                println!(
                    "{} ✓ {} ({}s)",
                    style("→").cyan(),
                    result.store_path,
                    result.duration_secs
                );
            } else {
                println!(
                    "{} ✗ {} ({})",
                    style("→").yellow(),
                    result.store_path,
                    result.error.as_deref().unwrap_or("unknown error")
                );
            }

            result
        }
    });

    // Execute all uploads concurrently
    let results = stream::iter(upload_futures)
        .buffer_unordered(config.concurrency)
        .collect::<Vec<_>>()
        .await;

    // Print summary
    let successful = results.iter().filter(|r| r.success).count();
    let failed = results.iter().filter(|r| !r.success).count();
    let total_time: u64 = results.iter().map(|r| r.duration_secs).max().unwrap_or(0);

    println!(
        "\n{} Upload summary: {} successful, {} failed ({}s total)",
        style("→").cyan(),
        style(successful).green(),
        if failed > 0 {
            style(failed).red().to_string()
        } else {
            style(failed).green().to_string()
        },
        total_time
    );

    Ok(results)
}

/// Upload a single store path
async fn upload_single(task: &UploadTask) -> Result<()> {
    use crate::upload;

    // Get token from environment
    let token = std::env::var("FLAKECACHE_TOKEN")
        .map_err(|_| anyhow::anyhow!("FLAKECACHE_TOKEN env var not set"))?;

    // Use the reusable upload function from the upload module
    upload::upload_single_store_path(
        &task.store_path,
        &task.cache_name,
        &task.api_url,
        &token,
    )
    .await
}

/// Calculate optimal concurrency level based on system resources
pub fn calculate_optimal_concurrency() -> usize {
    let cpu_count = num_cpus::get();

    // Heuristic: 1-2x CPU count, but cap at reasonable limits
    let optimal = (cpu_count as f32 * 1.5).ceil() as usize;
    optimal.max(2).min(16)
}

/// Check network bandwidth and adjust concurrency if needed
///
/// This function detects available bandwidth and recommends an optimal
/// concurrency level. Environment variable overrides are supported:
/// - FLAKECACHE_CONCURRENCY: Explicit concurrency level
/// - FLAKECACHE_BANDWIDTH_MBPS: Manually specified bandwidth
pub async fn adaptive_concurrency() -> usize {
    use crate::bandwidth;

    match bandwidth::get_adaptive_concurrency().await {
        Ok(concurrency) => concurrency,
        Err(_) => calculate_optimal_concurrency(), // Fallback on error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ParallelUploadConfig::default();
        assert_eq!(config.concurrency, 4);
        assert_eq!(config.timeout_secs, 300);
    }

    #[test]
    fn test_optimal_concurrency() {
        let concurrency = calculate_optimal_concurrency();
        assert!(concurrency >= 2 && concurrency <= 16);
    }

    #[tokio::test]
    async fn test_empty_uploads() {
        let config = ParallelUploadConfig::default();
        let results = upload_parallel(vec![], config).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_adaptive_concurrency() {
        // Test that adaptive_concurrency returns a reasonable value
        let concurrency = adaptive_concurrency().await;
        assert!(concurrency >= 1 && concurrency <= 16);
    }

    #[tokio::test]
    async fn test_adaptive_concurrency_with_override() {
        std::env::set_var("FLAKECACHE_CONCURRENCY", "6");
        let concurrency = adaptive_concurrency().await;
        assert_eq!(concurrency, 6);
        std::env::remove_var("FLAKECACHE_CONCURRENCY");
    }
}
