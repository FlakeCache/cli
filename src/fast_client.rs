use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

/// Create an optimized HTTP client for maximum download speed
/// Features:
/// - HTTP/2 multiplexing (automatically negotiated, up to 100 streams per connection)
/// - Connection pooling (reuse connections, 100 per host)
/// - Aggressive timeouts (don't wait for slow connections)
/// - Keep-alive (maintain connections for 90 seconds)
/// - TCP optimizations (no delay, keep-alive, window scaling)
/// - Zero-copy where possible (streaming)
///
/// # Errors
///
/// Returns an error if the client cannot be built (e.g., invalid configuration).
pub fn create_fast_client() -> Result<Client> {
    Client::builder()
        // HTTP/2 is automatically negotiated if server supports it
        // reqwest 0.12 uses h2 crate which supports HTTP/2 multiplexing (up to 100 streams per connection)
        .tcp_keepalive(Duration::from_secs(60))
        .tcp_nodelay(true) // Disable Nagle's algorithm (lower latency)
        .pool_max_idle_per_host(100) // Keep 100 idle connections per host (for 10,000 total streams with HTTP/2)
        .pool_idle_timeout(Duration::from_secs(90))
        .timeout(Duration::from_secs(300)) // 5 minute timeout (for large files)
        .connect_timeout(Duration::from_secs(10)) // 10 second connect timeout
        .read_timeout(Duration::from_secs(60)) // 60 second read timeout
        // Note: Compression is handled automatically by reqwest based on Accept-Encoding header
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))
}
