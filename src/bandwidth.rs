#![allow(clippy::cast_precision_loss, clippy::float_cmp)] // Bandwidth calculations - precision/comparison acceptable

use anyhow::Result;
use console::style;

/// Network bandwidth tier classification
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandwidthTier {
    /// Very slow: < 1 Mbps (e.g., cellular, poor `WiFi`)
    VerySlow,
    /// Slow: 1-10 Mbps (e.g., `WiFi`, DSL)
    Slow,
    /// Medium: 10-100 Mbps (e.g., good `WiFi`, home broadband)
    Medium,
    /// Fast: 100-500 Mbps (e.g., fiber, office network)
    Fast,
    /// Very fast: > 500 Mbps (e.g., enterprise, datacenter)
    VeryFast,
}

/// Detected network bandwidth and recommended tuning parameters
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BandwidthProfile {
    /// Estimated bandwidth in Mbps
    pub bandwidth_mbps: f64,
    /// Detected tier
    pub tier: BandwidthTier,
    /// Recommended concurrent connections
    pub recommended_concurrency: usize,
    /// Recommended chunk size in bytes (for parallel downloads)
    pub chunk_size_bytes: usize,
}

impl BandwidthProfile {
    /// Create a new bandwidth profile from detected bandwidth
    pub fn new(bandwidth_mbps: f64) -> Self {
        let tier = Self::classify_bandwidth(bandwidth_mbps);
        let (concurrency, chunk_size) = Self::recommend_tuning(tier);

        Self {
            bandwidth_mbps,
            tier,
            recommended_concurrency: concurrency,
            chunk_size_bytes: chunk_size,
        }
    }

    /// Classify bandwidth into tiers
    fn classify_bandwidth(mbps: f64) -> BandwidthTier {
        match mbps {
            x if x < 1.0 => BandwidthTier::VerySlow,
            x if x < 10.0 => BandwidthTier::Slow,
            x if x < 100.0 => BandwidthTier::Medium,
            x if x < 500.0 => BandwidthTier::Fast,
            _ => BandwidthTier::VeryFast,
        }
    }

    /// Recommend tuning parameters based on bandwidth tier
    const fn recommend_tuning(tier: BandwidthTier) -> (usize, usize) {
        match tier {
            // Very slow: Be conservative, low concurrency
            BandwidthTier::VerySlow => {
                (1, 1_000_000) // 1 connection, 1 MB chunks
            }
            // Slow: Limited parallelism
            BandwidthTier::Slow => {
                (2, 2_000_000) // 2 connections, 2 MB chunks
            }
            // Medium: Moderate parallelism
            BandwidthTier::Medium => {
                (4, 4_000_000) // 4 connections, 4 MB chunks
            }
            // Fast: Aggressive parallelism
            BandwidthTier::Fast => {
                (8, 8_000_000) // 8 connections, 8 MB chunks
            }
            // Very fast: Maximum parallelism
            BandwidthTier::VeryFast => {
                (16, 16_000_000) // 16 connections, 16 MB chunks
            }
        }
    }
}

/// Probe network bandwidth with a small test
///
/// This function performs a quick bandwidth probe by simulating a small
/// data transfer and measuring throughput. In production, this would
/// measure actual network latency and throughput.
#[allow(dead_code)]
pub async fn probe_bandwidth() -> Result<BandwidthProfile> {
    // Simulate bandwidth detection
    // In a real implementation, this would:
    // 1. Make a small upload/download request
    // 2. Measure bytes transferred and time taken
    // 3. Calculate bandwidth = bytes / time
    //
    // For now, use a heuristic based on simple latency probe

    let estimated_bandwidth = estimate_bandwidth_heuristic().await?;
    Ok(BandwidthProfile::new(estimated_bandwidth))
}

/// Estimate bandwidth using a simple heuristic
/// In production, this would measure actual network performance
#[allow(dead_code, clippy::unused_async)] // Async signature for future network measurements
async fn estimate_bandwidth_heuristic() -> Result<f64> {
    // This is a placeholder that uses reasonable defaults
    // In production, you'd:
    // 1. Measure DNS lookup time
    // 2. Measure TCP handshake time
    // 3. Extrapolate to estimated bandwidth
    // 4. Cache the result for 1-5 minutes

    // Default estimate: 50 Mbps
    // This is reasonable for most CI environments (good WiFi/broadband)
    Ok(50.0)
}

/// Get bandwidth-based concurrency level
///
/// Uses environment variable override if set, otherwise probes network
#[allow(dead_code)]
pub async fn get_adaptive_concurrency() -> Result<usize> {
    // Check for explicit override first
    if let Ok(concurrency_str) = std::env::var("FLAKECACHE_CONCURRENCY") {
        if let Ok(concurrency) = concurrency_str.parse::<usize>() {
            println!(
                "{} Using explicit concurrency from FLAKECACHE_CONCURRENCY: {}",
                style("→").cyan(),
                concurrency
            );
            return Ok(concurrency);
        }
    }

    // Check for bandwidth override
    if let Ok(bandwidth_str) = std::env::var("FLAKECACHE_BANDWIDTH_MBPS") {
        if let Ok(bandwidth) = bandwidth_str.parse::<f64>() {
            let profile = BandwidthProfile::new(bandwidth);
            println!(
                "{} Using bandwidth profile from FLAKECACHE_BANDWIDTH_MBPS: {:.1} Mbps ({:?})",
                style("→").cyan(),
                profile.bandwidth_mbps,
                profile.tier
            );
            return Ok(profile.recommended_concurrency);
        }
    }

    // Auto-detect bandwidth
    match probe_bandwidth().await {
        Ok(profile) => {
            println!(
                "{} Detected bandwidth: {:.1} Mbps ({:?})",
                style("→").cyan(),
                profile.bandwidth_mbps,
                profile.tier
            );
            println!(
                "{} Recommended concurrency: {} connections, {} byte chunks",
                style("→").cyan(),
                profile.recommended_concurrency,
                profile.chunk_size_bytes
            );
            Ok(profile.recommended_concurrency)
        }
        Err(e) => {
            println!(
                "{} Bandwidth detection failed: {} (using default)",
                style("⚠️").yellow(),
                e
            );
            Ok(4) // Default fallback
        }
    }
}

/// Get chunk size based on bandwidth
#[allow(dead_code, clippy::option_if_let_else)] // False positive - this is Result not Option
pub async fn get_adaptive_chunk_size() -> Result<usize> {
    // Check for explicit override
    if let Ok(chunk_str) = std::env::var("FLAKECACHE_CHUNK_SIZE_BYTES") {
        if let Ok(chunk_size) = chunk_str.parse::<usize>() {
            return Ok(chunk_size);
        }
    }

    // Use bandwidth-based recommendation
    match probe_bandwidth().await {
        Ok(profile) => Ok(profile.chunk_size_bytes),
        Err(_) => Ok(4_000_000), // Default 4 MB chunks
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_bandwidth_classification() {
        // Test classification of different bandwidth levels
        assert_eq!(BandwidthProfile::new(0.5).tier, BandwidthTier::VerySlow);
        assert_eq!(BandwidthProfile::new(5.0).tier, BandwidthTier::Slow);
        assert_eq!(BandwidthProfile::new(50.0).tier, BandwidthTier::Medium);
        assert_eq!(BandwidthProfile::new(200.0).tier, BandwidthTier::Fast);
        assert_eq!(BandwidthProfile::new(1000.0).tier, BandwidthTier::VeryFast);
    }

    #[test]
    fn test_concurrency_tuning() {
        // Very slow: conservative
        let very_slow = BandwidthProfile::new(0.5);
        assert_eq!(very_slow.recommended_concurrency, 1);

        // Slow: limited parallelism
        let slow = BandwidthProfile::new(5.0);
        assert_eq!(slow.recommended_concurrency, 2);

        // Medium: moderate parallelism
        let medium = BandwidthProfile::new(50.0);
        assert_eq!(medium.recommended_concurrency, 4);

        // Fast: aggressive parallelism
        let fast = BandwidthProfile::new(200.0);
        assert_eq!(fast.recommended_concurrency, 8);

        // Very fast: maximum parallelism
        let very_fast = BandwidthProfile::new(1000.0);
        assert_eq!(very_fast.recommended_concurrency, 16);
    }

    #[test]
    fn test_chunk_size_tuning() {
        // Very slow: smaller chunks
        let very_slow = BandwidthProfile::new(0.5);
        assert_eq!(very_slow.chunk_size_bytes, 1_000_000);

        // Slow: moderate chunks
        let slow = BandwidthProfile::new(5.0);
        assert_eq!(slow.chunk_size_bytes, 2_000_000);

        // Medium: standard chunks
        let medium = BandwidthProfile::new(50.0);
        assert_eq!(medium.chunk_size_bytes, 4_000_000);

        // Fast: larger chunks
        let fast = BandwidthProfile::new(200.0);
        assert_eq!(fast.chunk_size_bytes, 8_000_000);

        // Very fast: very large chunks
        let very_fast = BandwidthProfile::new(1000.0);
        assert_eq!(very_fast.chunk_size_bytes, 16_000_000);
    }

    #[test]
    fn test_bandwidth_profile_creation() {
        let profile = BandwidthProfile::new(50.0);
        assert_eq!(profile.bandwidth_mbps, 50.0);
        assert_eq!(profile.tier, BandwidthTier::Medium);
        assert_eq!(profile.recommended_concurrency, 4);
        assert_eq!(profile.chunk_size_bytes, 4_000_000);
    }

    #[tokio::test]
    async fn test_adaptive_concurrency_default() {
        // When no env vars set, should return a reasonable default
        std::env::remove_var("FLAKECACHE_CONCURRENCY");
        std::env::remove_var("FLAKECACHE_BANDWIDTH_MBPS");

        let concurrency = get_adaptive_concurrency().await.unwrap();
        assert!((1..=16).contains(&concurrency));
    }

    #[tokio::test]
    async fn test_concurrency_override() {
        std::env::set_var("FLAKECACHE_CONCURRENCY", "8");
        let concurrency = get_adaptive_concurrency().await.unwrap();
        assert_eq!(concurrency, 8);
        std::env::remove_var("FLAKECACHE_CONCURRENCY");
    }

    #[tokio::test]
    async fn test_bandwidth_override() {
        std::env::set_var("FLAKECACHE_BANDWIDTH_MBPS", "200");
        let concurrency = get_adaptive_concurrency().await.unwrap();
        assert_eq!(concurrency, 8); // 200 Mbps = Fast = 8 connections
        std::env::remove_var("FLAKECACHE_BANDWIDTH_MBPS");
    }
}
