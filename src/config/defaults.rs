//! Default configuration values

/// Default API server URL
pub fn default_api_url() -> String {
    "https://c.flakecache.com".to_string()
}

/// Default request timeout in seconds
pub fn default_timeout() -> u64 {
    300 // 5 minutes
}

/// Default parallelism (number of CPU cores)
pub fn default_parallelism() -> usize {
    num_cpus::get()
}

/// Default chunk size for uploads (16 MB)
pub const DEFAULT_CHUNK_SIZE: usize = 16 * 1024 * 1024;

/// Default max retries for failed requests
pub const DEFAULT_MAX_RETRIES: usize = 3;

/// Default retry backoff base in milliseconds
pub const DEFAULT_BACKOFF_BASE_MS: u64 = 100;

/// Maximum concurrent requests
pub const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 10;
