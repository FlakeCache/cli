use anyhow::Result;
use crate::auth;
use crate::chunked_download;
use crate::fast_client;
use console::style;
use reqwest::Client;
use std::path::PathBuf;
use tokio::fs::File as TokioFile;
use tokio::io::AsyncWriteExt;
use futures::StreamExt;

pub async fn download(
    cache: &str,
    hash: Option<&str>,
    store_path: Option<&str>,
    output: &str,
    api_url: &str,
) -> Result<()> {
    println!("{}", style("=== Downloading from FlakeCache ===\n").bold().cyan());
    
    let token = auth::load_token()?
        .or_else(|| std::env::var("FLAKECACHE_TOKEN").ok())
        .ok_or_else(|| anyhow::anyhow!("No token found. Run 'flakecache login' or set FLAKECACHE_TOKEN env var"))?;
    
    // Use optimized HTTP client for maximum speed (HTTP/2, connection pooling, etc.)
    let client = fast_client::create_fast_client()?;
    
    // Determine what to download
    let nar_hash = if let Some(h) = hash {
        h.to_string()
    } else if let Some(sp) = store_path {
        // Query NARInfo to get hash (standard Nix cache protocol)
        let narinfo_url = format!("{}/{}/{}.narinfo", api_url, cache, sp);
        let response = client
            .get(&narinfo_url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to fetch NARInfo: {}", response.status()));
        }
        
        let narinfo_text = response.text().await?;
        // Parse NARInfo to extract NAR hash
        // Format: NarHash: sha256:abc123...
        let nar_hash_line = narinfo_text
            .lines()
            .find(|line| line.starts_with("NarHash:"))
            .ok_or_else(|| anyhow::anyhow!("Invalid NARInfo format"))?;
        
        nar_hash_line
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Invalid NARHash format"))?
            .strip_prefix("sha256:")
            .ok_or_else(|| anyhow::anyhow!("Invalid NARHash format"))?
            .to_string()
    } else {
        return Err(anyhow::anyhow!("Must provide either --hash or --store-path"));
    };
    
    // Determine output file path
    let output_path = PathBuf::from(output);
    let output_file = if output_path.is_dir() {
        output_path.join(format!("{}.nar.xz", nar_hash))
    } else {
        output_path
    };
    
    // Create parent directory if needed
    if let Some(parent) = output_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    
    println!("{} Downloading NAR: {}", style("→").cyan(), nar_hash);
    println!("{} Output: {}", style("→").cyan(), output_file.display());
    println!();
    
    // Try presigned URL first (direct storage access, fastest)
    // Fall back to API endpoint if presigned URL not available
    let download_url = match get_presigned_url(&client, api_url, cache, &nar_hash, &token).await? {
        Some(presigned) => {
            println!("{} Using presigned URL for direct storage access (fastest)...", style("⚡").green());
            presigned
        }
        None => {
            // Use standard Nix cache protocol: {host}/{cache}/nar/{hash}.nar.xz
            format!("{}/{}/nar/{}.nar.xz", api_url, cache, nar_hash)
        }
    };
    
    let mut response = client
        .get(&download_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/x-nix-archive")
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Download failed: {}", response.status()));
    }
    
    // Get content length if available (for progress)
    let content_length = response.content_length();
    
    // For large files (>10MB), use chunked parallel download
    // This saturates multi-gigabit connections with 200 parallel threads downloading 4MB chunks
    const CHUNKED_THRESHOLD: u64 = 10 * 1_048_576; // 10MB (lower threshold for faster downloads)
    
    if let Some(size) = content_length {
        if size > CHUNKED_THRESHOLD {
            println!("{} Large file detected ({}MB), using ultra-fast chunked parallel download...", 
                style("⚡").cyan(), size / 1_048_576);
            println!("   {} Starting with 200 parallel connections, scaling up to 500 if bandwidth allows", 
                style("→").cyan());
            
            // Use chunked downloader (200 parallel threads, 4MB chunks, HTTP/2)
            return chunked_download::download_chunked(
                &client,
                &download_url,
                &token,
                &output_file,
                size,
                200, // 200 parallel threads (aggressive for maximum speed)
            ).await;
        }
    }
    
    // For smaller files, use streaming (simpler, less overhead)
    let mut downloaded_bytes = 0u64;
    
    // Open output file
    let mut file = TokioFile::create(&output_file).await?;
    
    // Stream chunks as they arrive from backend
    // Backend may still be downloading from storage, but we receive chunks immediately
    let mut stream = response.bytes_stream();
    
    println!("{} Streaming download (chunks as they arrive from backend)...", style("⬇️").cyan());
    
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let chunk_size = chunk.len() as u64;
        
        // Write chunk immediately to disk (backend hasn't fully downloaded to edge yet)
        file.write_all(&chunk).await?;
        downloaded_bytes += chunk_size;
        
        // Show progress if we know total size
        if let Some(total) = content_length {
            let percent = (downloaded_bytes * 100) / total;
            let downloaded_mb = downloaded_bytes as f64 / 1_000_000.0;
            let total_mb = total as f64 / 1_000_000.0;
            
            // Update progress on same line
            print!("\r  {} {:.1} MB / {:.1} MB ({}%)", 
                style("→").cyan(), 
                downloaded_mb, 
                total_mb, 
                percent
            );
            use std::io::Write;
            std::io::stdout().flush().ok();
        } else {
            // Unknown size, just show bytes downloaded
            let downloaded_mb = downloaded_bytes as f64 / 1_000_000.0;
            print!("\r  {} {:.1} MB downloaded", 
                style("→").cyan(), 
                downloaded_mb
            );
            use std::io::Write;
            std::io::stdout().flush().ok();
        }
    }
    
    // Flush and sync file
    file.sync_all().await?;
    
    println!(); // New line after progress
    println!("{} Download complete: {}", style("✓").green(), output_file.display());
    
    if let Ok(metadata) = tokio::fs::metadata(&output_file).await {
        let size_mb = metadata.len() as f64 / 1_000_000.0;
        println!("{} File size: {:.1} MB", style("→").cyan(), size_mb);
    }
    
    Ok(())
}

/// Try to get presigned URL for direct storage access (fastest - bypasses API)
/// Uses CBOR control channel to get presigned URL from S3/Tigris
async fn get_presigned_url(
    client: &Client,
    api_url: &str,
    cache: &str,
    nar_hash: &str,
    token: &str,
) -> Result<Option<String>> {
    // Try to get presigned URL from CBOR control channel
    let presigned_url = format!("{}/api/v2/cbor/{}/nar/{}/presigned", api_url, cache, nar_hash);
    
    match client
        .get(&presigned_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/cbor")
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            // Try CBOR first, fall back to JSON
            let body = response.bytes().await?;
            if let Ok(cbor_data) = ciborium::from_reader::<serde_json::Value, _>(&body[..]) {
                if let Some(url) = cbor_data.get("url").and_then(|v| v.as_str()) {
                    return Ok(Some(url.to_string()));
                }
            }
            // Fall back to JSON
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
                if let Some(url) = json.get("url").and_then(|v| v.as_str()) {
                    return Ok(Some(url.to_string()));
                }
            }
            Ok(None)
        }
        _ => Ok(None), // Presigned URL not available, fall back to API
    }
}
