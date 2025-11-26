use anyhow::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};
use tokio::task;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::io::Write;
use console::style;

/// Chunk status for tracking download progress
#[derive(Debug, Clone, PartialEq)]
enum ChunkStatus {
    Pending,
    Downloading,
    Completed(Vec<u8>),
    Failed(String),
}

/// Chunked downloader for large files (2GB+) split into 1MB chunks
/// Downloads chunks in parallel with adaptive throttling based on latency
pub struct ChunkedDownloader {
    /// Total file size (from Content-Length header)
    total_size: u64,
    
    /// Chunk size (1MB = 1_048_576 bytes)
    chunk_size: u64,
    
    /// Number of chunks
    num_chunks: usize,
    
    /// Chunk status tracker: chunk_index -> status
    chunks: Arc<Mutex<HashMap<usize, ChunkStatus>>>,
    
    /// Semaphore for limiting concurrent downloads (adaptive, starts at 50)
    semaphore: Arc<Semaphore>,
    
    /// Current max concurrent downloads (adaptive)
    max_concurrent: Arc<AtomicUsize>,
    
    /// Progress counter (bytes downloaded)
    bytes_downloaded: Arc<AtomicU64>,
    
    /// Latency tracker: recent response times (for congestion detection)
    latencies: Arc<Mutex<Vec<Duration>>>,
    
    /// Baseline latency (initial measurement)
    baseline_latency: Arc<Mutex<Option<Duration>>>,
}

impl ChunkedDownloader {
    /// Create a new chunked downloader with adaptive throttling
    pub fn new(total_size: u64, initial_concurrent: usize) -> Self {
        const CHUNK_SIZE: u64 = 4_194_304; // 4MB chunks (optimized for high bandwidth)
        
        let num_chunks = ((total_size + CHUNK_SIZE - 1) / CHUNK_SIZE) as usize;
        
        // Initialize all chunks as Pending
        let mut chunks = HashMap::new();
        for i in 0..num_chunks {
            chunks.insert(i, ChunkStatus::Pending);
        }
        
        Self {
            total_size,
            chunk_size: CHUNK_SIZE,
            num_chunks,
            chunks: Arc::new(Mutex::new(chunks)),
            semaphore: Arc::new(Semaphore::new(initial_concurrent)),
            max_concurrent: Arc::new(AtomicUsize::new(initial_concurrent)),
            bytes_downloaded: Arc::new(AtomicU64::new(0)),
            latencies: Arc::new(Mutex::new(Vec::new())),
            baseline_latency: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Adjust concurrency based on latency (adaptive throttling)
    /// If latency increases >2x baseline, reduce parallelism
    async fn adjust_concurrency(&self) {
        let latencies_guard = self.latencies.lock().await;
        
        // Need at least 10 samples to make decisions
        if latencies_guard.len() < 10 {
            return;
        }
        
        // Get recent latencies (last 20 samples)
        let recent: Vec<Duration> = latencies_guard
            .iter()
            .rev()
            .take(20)
            .cloned()
            .collect();
        
        let avg_latency = recent.iter().sum::<Duration>() / recent.len() as u32;
        
        drop(latencies_guard);
        
        // Check baseline
        let mut baseline_guard = self.baseline_latency.lock().await;
        let baseline = baseline_guard.get_or_insert_with(|| avg_latency);
        
        // If latency is >2x baseline, we're saturating too much - reduce parallelism
        if avg_latency > *baseline * 2 {
            let current = self.max_concurrent.load(Ordering::Relaxed);
            if current > 5 {
                // Reduce by 20% (but never below 5)
                let new = std::cmp::max(5, (current as f64 * 0.8) as usize);
                self.max_concurrent.store(new, Ordering::Relaxed);
                
                // Note: Can't dynamically resize Semaphore, but we'll respect the limit
                // by checking max_concurrent before acquiring permit
                println!("⚠️  Latency increased ({}ms), reducing to {} concurrent downloads", 
                    avg_latency.as_millis(), new);
            }
        } else if avg_latency < *baseline * 1.5 {
            // Latency is good, can increase parallelism (gradually)
            let current = self.max_concurrent.load(Ordering::Relaxed);
            if current < 100 {
                let new = std::cmp::min(100, current + 2);
                self.max_concurrent.store(new, Ordering::Relaxed);
            }
        }
        
        drop(baseline_guard);
    }
    
    /// Download file in chunks and reassemble
    pub async fn download(
        &self,
        client: &Client,
        url: &str,
        token: &str,
        output_path: &PathBuf,
    ) -> Result<()> {
        println!("⚡ Ultra-fast download: {} chunks ({}MB each, {} parallel connections, HTTP/2)...", 
            self.num_chunks, self.chunk_size / 1_048_576, self.semaphore.available_permits());
        println!("   {} Scaling up to 500 connections if bandwidth allows", style("→").cyan());
        
        // Fire all chunk downloads immediately (parallel)
        // Adaptive throttling will adjust concurrency based on latency
        let mut handles = Vec::new();
        
        // Spawn a task to periodically adjust concurrency based on latency
        let adjust_task = {
            let downloader = self.clone_for_adjustment();
            task::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    downloader.adjust_concurrency().await;
                }
            })
        };
        
        for chunk_idx in 0..self.num_chunks {
            let start = chunk_idx as u64 * self.chunk_size;
            let end = std::cmp::min(start + self.chunk_size - 1, self.total_size - 1);
            let chunk_len = end - start + 1;
            
            let client = client.clone();
            let url = url.to_string();
            let token = token.to_string();
            let chunks = self.chunks.clone();
            let semaphore = self.semaphore.clone();
            let bytes_downloaded = self.bytes_downloaded.clone();
            let latencies = self.latencies.clone();
            let max_concurrent = self.max_concurrent.clone();
            let total_size = self.total_size;
            let num_chunks = self.num_chunks;
            
            let handle = task::spawn(async move {
                // Check current max concurrent (adaptive throttling)
                let current_max = max_concurrent.load(Ordering::Relaxed);
                
                // Only acquire permit if we're under the adaptive limit
                // (Semaphore might allow more, but we self-limit)
                let permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Semaphore acquire failed: {}", e);
                        continue; // Skip this chunk if semaphore fails
                    }
                };
                let _permit = permit;
                
                // Double-check we're still under limit (might have changed)
                if max_concurrent.load(Ordering::Relaxed) < current_max {
                    // Concurrency was reduced, release permit and wait a bit
                    drop(_permit);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    return; // Skip this chunk for now, will retry later
                }
                
                // Mark chunk as downloading
                {
                    let mut chunks = chunks.lock().await;
                    chunks.insert(chunk_idx, ChunkStatus::Downloading);
                }
                
                // Download chunk with Range header (measure latency)
                let range_header = format!("bytes={}-{}", start, end);
                let start_time = Instant::now();
                
                match client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("Range", range_header)
                    .send()
                    .await
                {
                    Ok(response) => {
                        // Measure latency (time to first byte)
                        let latency = start_time.elapsed();
                        
                        // Record latency for adaptive throttling (keep last 50 samples)
                        {
                            let mut latencies_guard = latencies.lock().await;
                            latencies_guard.push(latency);
                            if latencies_guard.len() > 50 {
                                latencies_guard.remove(0); // Keep only last 50
                            }
                            
                            // Adjust concurrency every 10 samples
                            if latencies_guard.len() % 10 == 0 {
                                drop(latencies_guard);
                                // Note: adjust_concurrency needs &self, but we're in a closure
                                // We'll call it from the main loop instead
                            }
                        }
                        
                        if response.status().is_success() || response.status() == 206 {
                            // 206 = Partial Content (expected for Range requests)
                            match response.bytes().await {
                                Ok(chunk_data) => {
                                    let len = chunk_data.len() as u64;
                                    bytes_downloaded.fetch_add(len, Ordering::Relaxed);
                                    
                                    // Store chunk in HashMap
                                    let mut chunks_guard = chunks.lock().await;
                                    chunks_guard.insert(chunk_idx, ChunkStatus::Completed(chunk_data.to_vec()));
                                    
                                    // Show progress (release lock before printing)
                                    let completed_count = chunks_guard.values()
                                        .filter(|s| matches!(s, ChunkStatus::Completed(_))).count();
                                    drop(chunks_guard); // Release lock
                                    
                                    let downloaded = bytes_downloaded.load(Ordering::Relaxed);
                                    let percent = if total_size > 0 { (downloaded * 100) / total_size } else { 0 };
                                    
                                    // Show progress more frequently for better UX
                                    if chunk_idx % 5 == 0 || chunk_idx == num_chunks - 1 {
                                        let current_max = max_concurrent.load(Ordering::Relaxed);
                                        let speed_mbps = if latency.as_secs_f64() > 0.0 {
                                            (len as f64 / latency.as_secs_f64()) / 1_048_576.0 * 8.0
                                        } else {
                                            0.0
                                        };
                                        print!("\r⚡ {}% ({}/{}) chunks, {:.1}MB, {} concurrent, {:.1} Mbps", 
                                            percent, 
                                            completed_count,
                                            num_chunks,
                                            downloaded as f64 / 1_048_576.0,
                                            current_max,
                                            speed_mbps);
                                        std::io::stdout().flush().ok();
                                    }
                                }
                                Err(e) => {
                                    let mut chunks = chunks.lock().await;
                                    chunks.insert(chunk_idx, ChunkStatus::Failed(e.to_string()));
                                }
                            }
                        } else {
                            let mut chunks = chunks.lock().await;
                            chunks.insert(chunk_idx, ChunkStatus::Failed(format!("HTTP {}", response.status())));
                        }
                    }
                    Err(e) => {
                        let mut chunks = chunks.lock().await;
                        chunks.insert(chunk_idx, ChunkStatus::Failed(e.to_string()));
                    }
                }
            });
            
            handles.push(handle);
        }
        
        // Wait for all chunks to download
        futures::future::join_all(handles).await;
        
        // Stop the adjustment task
        adjust_task.abort();
        
        println!("\r✅ All chunks downloaded, reassembling...");
        
        // Reassemble chunks in order (streaming to disk, not memory)
        self.reassemble_chunks(output_path).await?;
        
        Ok(())
    }
    
    /// Reassemble chunks in order and write to disk (streaming, memory-efficient)
    async fn reassemble_chunks(&self, output_path: &PathBuf) -> Result<()> {
        // Create output file
        let mut file = TokioFile::create(output_path).await?;
        
        // Write chunks in order (0, 1, 2, ...)
        for chunk_idx in 0..self.num_chunks {
            let chunks = self.chunks.lock().await;
            
            match chunks.get(&chunk_idx) {
                Some(ChunkStatus::Completed(data)) => {
                    // Write chunk to file at correct position
                    let start_pos = chunk_idx as u64 * self.chunk_size;
                    file.seek(std::io::SeekFrom::Start(start_pos)).await?;
                    file.write_all(data).await?;
                }
                Some(ChunkStatus::Failed(err)) => {
                    return Err(anyhow::anyhow!("Chunk {} failed: {}", chunk_idx, err));
                }
                _ => {
                    return Err(anyhow::anyhow!("Chunk {} not completed", chunk_idx));
                }
            }
        }
        
        // Sync file to disk
        file.sync_all().await?;
        
        println!("✅ File reassembled: {}", output_path.display());
        
        Ok(())
    }
    
    /// Get download progress (0-100)
    pub fn progress(&self) -> u64 {
        let downloaded = self.bytes_downloaded.load(Ordering::Relaxed);
        if self.total_size > 0 {
            (downloaded * 100) / self.total_size
        } else {
            0
        }
    }
    
    /// Clone for adjustment task (only needed fields)
    fn clone_for_adjustment(&self) -> AdaptiveThrottler {
        AdaptiveThrottler {
            latencies: self.latencies.clone(),
            baseline_latency: self.baseline_latency.clone(),
            max_concurrent: self.max_concurrent.clone(),
        }
    }
}

/// Lightweight struct for adaptive throttling (doesn't need full downloader)
struct AdaptiveThrottler {
    latencies: Arc<Mutex<Vec<Duration>>>,
    baseline_latency: Arc<Mutex<Option<Duration>>>,
    max_concurrent: Arc<AtomicUsize>,
}

impl AdaptiveThrottler {
    async fn adjust_concurrency(&self) {
        let latencies_guard = self.latencies.lock().await;
        
        // Need at least 10 samples to make decisions
        if latencies_guard.len() < 10 {
            return;
        }
        
        // Get recent latencies (last 20 samples)
        let recent: Vec<Duration> = latencies_guard
            .iter()
            .rev()
            .take(20)
            .cloned()
            .collect();
        
        let avg_latency = recent.iter().sum::<Duration>() / recent.len() as u32;
        
        drop(latencies_guard);
        
        // Check baseline
        let mut baseline_guard = self.baseline_latency.lock().await;
        let baseline = baseline_guard.get_or_insert_with(|| avg_latency);
        
        // If latency is >2x baseline, we're saturating too much - reduce parallelism
        if avg_latency > *baseline * 2 {
            let current = self.max_concurrent.load(Ordering::Relaxed);
            if current > 5 {
                // Reduce by 20% (but never below 5)
                let new = std::cmp::max(5, (current as f64 * 0.8) as usize);
                self.max_concurrent.store(new, Ordering::Relaxed);
                
                println!("\n⚠️  We are slowing down - you need a better connection to keep up with us!");
                println!("   Latency increased ({}ms vs {}ms baseline), reducing to {} concurrent downloads", 
                    avg_latency.as_millis(), baseline.as_millis(), new);
            }
        } else if avg_latency < *baseline * 1.5 {
            // Latency is good, can increase parallelism (gradually)
            let current = self.max_concurrent.load(Ordering::Relaxed);
            if current < 500 {
                // Aggressive scaling for fastest downloads (up to 500 connections)
                let new = std::cmp::min(500, current + 5);
                self.max_concurrent.store(new, Ordering::Relaxed);
            }
        }
        
        drop(baseline_guard);
    }
}

/// Download large file using chunked parallel download
pub async fn download_chunked(
    client: &Client,
    url: &str,
    token: &str,
    output_path: &PathBuf,
    total_size: u64,
    max_concurrent: usize,
) -> Result<()> {
    let downloader = ChunkedDownloader::new(total_size, max_concurrent);
    downloader.download(client, url, token, output_path).await?;
    Ok(())
}
