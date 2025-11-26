use crate::auth;
use crate::cbor_client::CborClient;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Response for list command
#[derive(Debug, Serialize, Deserialize)]
pub struct StorePath {
    pub path: String,
    pub nar_hash: String,
    pub nar_size: u64,
    pub uploaded_at: String,
    pub uploaded_by: Option<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListResponse {
    pub paths: Vec<StorePath>,
    pub total: usize,
}

/// Response for inspect command
#[derive(Debug, Serialize, Deserialize)]
pub struct PathMetadata {
    pub path: String,
    pub nar_hash: String,
    pub nar_size: u64,
    pub compression: String,
    pub uploaded_at: String,
    pub uploaded_by: String,
    pub references: Vec<String>,
    pub deriver: Option<String>,
    pub system: Option<String>,
}

/// Response for stats command
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_size: u64,
    pub artifact_count: usize,
    pub hit_count: u64,
    pub miss_count: u64,
    pub bandwidth_saved: u64,
    pub period_days: u32,
}

/// Request for GC command
#[derive(Debug, Serialize, Deserialize)]
pub struct GcRequest {
    pub older_than_days: u32,
    pub dry_run: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GcResponse {
    pub paths_deleted: Vec<String>,
    pub total_deleted: usize,
    pub bytes_freed: u64,
}

/// List paths in cache
pub async fn list_paths(
    cache: &str,
    query: Option<String>,
    older_than: Option<String>,
    api_url: &str,
) -> Result<()> {
    let token = auth::load_token()?
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run 'flakecache login'"))?;

    let client = CborClient::new(api_url, &token);

    // Build query parameters
    let mut path = format!("/cache/{cache}/paths");
    let mut params = vec![];

    if let Some(q) = query {
        params.push(format!("query={}", urlencoding::encode(&q)));
    }

    if let Some(age) = older_than {
        params.push(format!("older_than={}", urlencoding::encode(&age)));
    }

    if !params.is_empty() {
        path = format!("{}?{}", path, params.join("&"));
    }

    println!("📦 Fetching cache contents...\n");

    let response: ListResponse = client.get(&path).await?;

    if response.paths.is_empty() {
        println!("No paths found in cache.");
        return Ok(());
    }

    println!("Found {} path(s):\n", response.total);

    for path in &response.paths {
        println!("📄 {}", path.path);
        println!("   Hash: {}", path.nar_hash);
        println!("   Size: {} bytes", format_bytes(path.nar_size));
        println!("   Uploaded: {}", path.uploaded_at);
        if let Some(ref by) = path.uploaded_by {
            println!("   By: {by}");
        }
        if !path.references.is_empty() {
            println!("   References: {} paths", path.references.len());
        }
        println!();
    }

    Ok(())
}

/// Inspect a specific store path
pub async fn inspect_path(cache: &str, store_path: &str, api_url: &str) -> Result<()> {
    let token = auth::load_token()?
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run 'flakecache login'"))?;

    let client = CborClient::new(api_url, &token);

    // Encode the store path for URL
    let encoded_path = urlencoding::encode(store_path);
    let path = format!("/cache/{cache}/inspect/{encoded_path}");

    println!("🔍 Fetching metadata for {store_path}...\n");

    let metadata: PathMetadata = client.get(&path).await?;

    println!("📄 Store Path: {}", metadata.path);
    println!("   NAR Hash: {}", metadata.nar_hash);
    println!("   NAR Size: {}", format_bytes(metadata.nar_size));
    println!("   Compression: {}", metadata.compression);
    println!("   Uploaded At: {}", metadata.uploaded_at);
    println!("   Uploaded By: {}", metadata.uploaded_by);

    if let Some(ref system) = metadata.system {
        println!("   System: {system}");
    }

    if let Some(ref deriver) = metadata.deriver {
        println!("   Deriver: {deriver}");
    }

    if !metadata.references.is_empty() {
        println!("\n   References ({}):", metadata.references.len());
        for reference in &metadata.references {
            println!("     - {reference}");
        }
    }

    Ok(())
}

/// Delete a store path from cache
pub async fn delete_path(cache: &str, store_path: &str, force: bool, api_url: &str) -> Result<()> {
    let token = auth::load_token()?
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run 'flakecache login'"))?;

    if !force {
        println!("⚠️  WARNING: You are about to delete:");
        println!("   {store_path}");
        println!("   from cache: {cache}");
        println!("\n   This action CANNOT be undone!");
        println!("\n   Use --force to confirm deletion.");
        return Ok(());
    }

    let encoded_path = urlencoding::encode(store_path);
    let path = format!("/cache/{cache}/delete/{encoded_path}");

    println!("🗑️  Deleting {store_path}...");

    // DELETE request
    let url = format!("{api_url}/api/v2/cbor{path}");
    let response = reqwest::Client::new()
        .delete(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Failed to delete: {}", response.status()));
    }

    println!("✅ Successfully deleted.");

    Ok(())
}

/// Garbage collect old paths
pub async fn gc_cache(cache: &str, older_than: &str, dry_run: bool, api_url: &str) -> Result<()> {
    let token = auth::load_token()?
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run 'flakecache login'"))?;

    // Parse duration (e.g., "30d" -> 30 days)
    let days = parse_duration_to_days(older_than)?;

    let client = CborClient::new(api_url, &token);

    let request = GcRequest {
        older_than_days: days,
        dry_run,
    };

    let path = format!("/cache/{cache}/gc");

    if dry_run {
        println!("🧹 Garbage collection (DRY RUN)");
    } else {
        println!("🧹 Garbage collection");
    }
    println!("   Cache: {cache}");
    println!("   Removing paths older than: {days} days\n");

    let response: GcResponse = client.post(&path, &request).await?;

    if dry_run {
        println!("Would delete {} path(s):", response.total_deleted);
    } else {
        println!("Deleted {} path(s):", response.total_deleted);
    }

    for path in &response.paths_deleted {
        println!("  - {path}");
    }

    println!("\n💾 Space freed: {}", format_bytes(response.bytes_freed));

    if dry_run {
        println!("\n(This was a dry run. Use without --dry-run to actually delete.)");
    }

    Ok(())
}

/// Show cache statistics
#[allow(clippy::cast_precision_loss)] // Precision loss acceptable for display percentages
pub async fn show_stats(cache: &str, period: &str, api_url: &str) -> Result<()> {
    let token = auth::load_token()?
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run 'flakecache login'"))?;

    let days = parse_duration_to_days(period)?;

    let client = CborClient::new(api_url, &token);

    let path = format!("/cache/{cache}/stats?period={days}d");

    println!("📊 Cache Statistics: {cache}");
    println!("   Period: {days} days\n");

    let stats: CacheStats = client.get(&path).await?;

    println!("📦 Total Size: {}", format_bytes(stats.total_size));
    println!("📄 Artifact Count: {}", stats.artifact_count);
    println!();

    let total_requests = stats.hit_count + stats.miss_count;
    let hit_rate = if total_requests > 0 {
        (stats.hit_count as f64 / total_requests as f64) * 100.0
    } else {
        0.0
    };

    println!("🎯 Cache Performance:");
    println!("   Hits: {}", stats.hit_count);
    println!("   Misses: {}", stats.miss_count);
    println!("   Hit Rate: {hit_rate:.1}%");
    println!();

    println!(
        "💰 Bandwidth Saved: {}",
        format_bytes(stats.bandwidth_saved)
    );

    Ok(())
}

/// Parse duration strings like "30d", "7d", "24h" to days
fn parse_duration_to_days(duration: &str) -> Result<u32> {
    if let Some(days_str) = duration.strip_suffix('d') {
        let days: u32 = days_str.parse()?;
        Ok(days)
    } else if let Some(hours_str) = duration.strip_suffix('h') {
        let hours: u32 = hours_str.parse()?;
        Ok(hours.div_ceil(24)) // Round up to days
    } else {
        Err(anyhow::anyhow!(
            "Invalid duration format. Use '30d' or '24h'"
        ))
    }
}

/// Format bytes in human-readable format
#[allow(clippy::cast_precision_loss)] // Precision loss acceptable for human-readable sizes
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
