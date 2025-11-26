use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

/// Cache for dependency graphs and build order between CI runs
/// Stored as CBOR for fast binary format
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DependencyCache {
    /// Timestamp when cache was created
    pub created_at: u64,
    
    /// Hash of derivations (for cache invalidation)
    pub derivations_hash: String,
    
    /// All store paths discovered
    pub paths: Vec<String>,
    
    /// Build order (topologically sorted paths)
    pub build_order: Vec<String>,
    
    /// Graph edges: path -> dependencies
    pub edges: HashMap<String, Vec<String>>,
    
    /// Cache status: path -> (exists_in_cache, last_checked)
    pub cache_status: HashMap<String, (bool, u64)>,
}

impl DependencyCache {
    /// Load cache from disk (CBOR format) - INSTANT, no parsing overhead
    pub fn load(cache_path: &Path) -> Result<Option<Self>> {
        if !cache_path.exists() {
            return Ok(None);
        }
        
        // INSTANT LOAD: Read entire file at once (faster than streaming for small files)
        let data = fs::read(cache_path)?;
        
        // Try CBOR first (fast binary, 3-5x faster than JSON)
        match cbor::decode(&data) {
            Ok(cache) => Ok(Some(cache)),
            Err(_) => {
                // Fall back to JSON for compatibility (slower but works)
                match serde_json::from_slice(&data) {
                    Ok(cache) => Ok(Some(cache)),
                    Err(_) => {
                        // Corrupted cache, delete it silently (don't error, just rebuild)
                        let _ = fs::remove_file(cache_path);
                        Ok(None)
                    }
                }
            }
        }
    }
    
    /// Save cache to disk (CBOR format for speed) - FAST binary encoding
    pub fn save(&self, cache_path: &Path) -> Result<()> {
        // Create parent directory if needed (async would be faster but sync is fine for cache)
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Encode as CBOR (fast binary format, 3-5x faster than JSON)
        let encoded = cbor::encode(self)?;
        
        // Atomic write: Write to temp file then rename (prevents corruption)
        let temp_path = cache_path.with_extension("tmp");
        fs::write(&temp_path, encoded)?;
        fs::rename(&temp_path, cache_path)?;
        
        Ok(())
    }
    
    /// Check if cache is still valid (derivations haven't changed)
    pub fn is_valid(&self, current_derivations_hash: &str) -> bool {
        self.derivations_hash == current_derivations_hash
    }
    
    /// Get cache age in seconds.
    /// 
    /// Returns 0 if system time is before UNIX epoch (should never happen in practice).
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default() // Fallback to 0 if time is before epoch (should never happen)
            .as_secs();
        
        if now > self.created_at {
            now - self.created_at
        } else {
            0
        }
    }
}

/// Get cache directory path (e.g., ~/.cache/flakecache or .flakecache/)
pub fn get_cache_dir() -> Result<PathBuf> {
    // Try .flakecache/ in repo root first (for CI, persists in workspace)
    if let Ok(repo_root) = find_repo_root() {
        let repo_cache = repo_root.join(".flakecache");
        if repo_cache.exists() || repo_root.exists() {
            return Ok(repo_cache);
        }
    }
    
    // Fall back to user cache directory
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find cache directory"))?
        .join("flakecache");
    
    Ok(cache_dir)
}

/// Get cache file path for a specific derivation hash
pub fn get_cache_file(derivations_hash: &str) -> Result<PathBuf> {
    let cache_dir = get_cache_dir()?;
    let filename = format!("deps-{}.cbor", derivations_hash);
    Ok(cache_dir.join(filename))
}

/// Compute hash of derivations for cache invalidation
pub fn hash_derivations(derivations: &[String]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    
    // Sort for consistent hashing
    let mut sorted = derivations.to_vec();
    sorted.sort();
    
    for drv in sorted {
        hasher.update(drv.as_bytes());
        hasher.update(b"\n");
    }
    
    let hash_bytes = hasher.finalize();
    hex::encode(&hash_bytes[..8]) // Use first 8 bytes as short hash
}

fn find_repo_root() -> Result<PathBuf> {
    let mut current_dir = std::env::current_dir()?;
    
    loop {
        // Check for git repo
        if current_dir.join(".git").exists() {
            return Ok(current_dir);
        }
        
        // Check for flake.nix (Nix flake)
        if current_dir.join("flake.nix").exists() {
            return Ok(current_dir);
        }
        
        // Move up one directory
        match current_dir.parent() {
            Some(parent) => current_dir = parent.to_path_buf(),
            None => return Err(anyhow::anyhow!("Could not find repo root")),
        }
    }
}
