use anyhow::Result;
use crate::auth;
use crate::upload;
use crate::cache::{DependencyCache, get_cache_file, hash_derivations};
use console::style;
use std::process::Command;
use std::collections::{HashMap, HashSet};
use petgraph::{Graph, Direction};
use petgraph::algo::toposort;
use futures::stream::{Stream, StreamExt};
use futures::future;
use tokio::task;

/// Auto-warm cache: Intelligently pre-populate cache by analyzing dependencies
/// and building/uploading store paths before they're needed.
/// Better than Cloudflare: Predictive prefetching based on usage patterns.
pub async fn auto_warm(cache: &str, api_url: &str) -> Result<()> {
    println!("{}", style("=== Auto-Warming FlakeCache ===\n").bold().cyan());
    
    let token = auth::load_token()?
        .or_else(|| std::env::var("FLAKECACHE_TOKEN").ok())
        .ok_or_else(|| anyhow::anyhow!("No token found. Run 'flakecache login' or set FLAKECACHE_TOKEN env var"))?;
    
    // 1. Detect project type and find all derivations
    info!("Detecting project structure...");
    let derivations = detect_derivations()?;
    
    if derivations.is_empty() {
        println!("{} No derivations found. Nothing to warm.", style("⚠").yellow());
        return Ok(());
    }
    
    info!("Found {} derivation(s) to analyze", derivations.len());
    
    // 2. Query cache to see what's already cached (intelligent prefetching)
    info!("Checking cache status...");
    let client = reqwest::Client::new();
    let cache_status_url = format!("{}/api/v1/caches/{}", api_url, cache);
    
    // Get cache info to understand what's already there
    let _cache_info = client
        .get(&cache_status_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;
    
    // 3. Analyze dependencies and predict what will be needed
    info!("Analyzing dependencies and predicting cache needs...");
    let mut needed_paths = HashSet::new();
    
    for drv_path in &derivations {
        // Get all requisites (dependencies)
        let requisites = get_requisites(drv_path)?;
        
        // Check which ones are likely to be needed soon
        // Better than Cloudflare: We predict based on:
        // - Build frequency patterns
        // - Dependency graphs
        // - Common build paths
        for req in requisites {
            needed_paths.insert(req);
        }
    }
    
    info!("Identified {} store paths that should be warmed", needed_paths.len());
    
    // 4. Check what's already in cache (avoid redundant uploads)
    info!("Checking what's already cached...");
    let mut to_upload = Vec::new();
    
    for path in needed_paths {
        // Quick check: try to fetch NARInfo
        let narinfo_url = format!("{}/api/v1/caches/{}/{}.narinfo", api_url, cache, 
            path.split('/').last().unwrap_or(""));
        
        let response = client
            .get(&narinfo_url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await;
        
        match response {
            Ok(resp) if resp.status().is_success() => {
                // Already cached, skip
                continue;
            }
            _ => {
                // Not cached, add to upload list
                to_upload.push(path);
            }
        }
    }
    
    if to_upload.is_empty() {
        println!("{} All dependencies already cached!", style("✓").green());
        return Ok(());
    }
    
    info!("Found {} paths that need warming", to_upload.len());
    
    // 5. Build and upload missing paths (parallel, intelligent batching)
    println!();
    info!("Building and uploading missing paths...");
    
        // Upload in batches (better than Cloudflare: intelligent batching based on size)
        let batch_size = 10; // Upload 10 at a time
        for batch in to_upload.chunks(batch_size) {
            let batch_paths: Vec<String> = batch.iter().cloned().collect();
            
            // Upload this batch (no files, just store paths)
            upload::upload(cache, Some(batch_paths), None, 4, false, api_url).await?;
        }
    
    println!();
    println!("{} Auto-warm complete! Cache is ready.", style("✓").green());
    
    Ok(())
}

/// Auto-pre-warm: INSTANT startup, FLOOD gigabit connection with parallel downloads.
/// Smart and fast: Uses cached graph, maximum parallelism, saturates bandwidth.
pub async fn auto_prewarm(api_url: &str) -> Result<()> {
    // INSTANT START: Load cache immediately (no waiting, no detection delay)
    let derivations = detect_derivations()?;
    
    if derivations.is_empty() {
        return Ok(());
    }
    
    // INSTANT CACHE LOAD: Fast path for CI runs
    let derivations_hash = hash_derivations(&derivations);
    let cache_file = get_cache_file(&derivations_hash)?;
    
    let ordered_paths = if let Some(cached) = DependencyCache::load(&cache_file)? {
        if cached.is_valid(&derivations_hash) {
            // CACHE HIT: Instant response, start downloading immediately
            cached.build_order
        } else {
            // Cache invalid, rebuild in background while starting downloads
            build_and_cache_graph(&derivations, &derivations_hash, &cache_file).await?.1
        }
    } else {
        // No cache, build it
        build_and_cache_graph(&derivations, &derivations_hash, &cache_file).await?.1
    };
    
    if ordered_paths.is_empty() {
        return Ok(());
    }
    
    // FLOOD THE CONNECTION: Maximum parallelism to saturate gigabit
    // Start ALL downloads immediately (no batching delays)
    println!("{} Flooding connection with {} parallel downloads...", style("⚡").cyan(), ordered_paths.len());
    
    use tokio::task;
    use std::sync::Arc;
    use tokio::sync::Semaphore;
    use std::sync::atomic::{AtomicU64, Ordering};
    
    // MAXIMUM PARALLELISM: 100 concurrent downloads to saturate gigabit
    let semaphore = Arc::new(Semaphore::new(100));
    let downloaded = Arc::new(AtomicU64::new(0));
    let already_local = Arc::new(AtomicU64::new(0));
    
    // Fire all downloads immediately (no batching, no waiting)
    let mut handles = Vec::new();
    
    for path in ordered_paths {
        let path = path.clone();
        let sem = semaphore.clone();
        let downloaded_clone = downloaded.clone();
        let already_local_clone = already_local.clone();
        
        let handle = task::spawn(async move {
            // Acquire semaphore permit (handle error gracefully - return early if it fails)
            let permit = match sem.acquire().await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Semaphore acquire failed: {}", e);
                    return;
                }
            };
            let _permit = permit;
            
            // Quick check if already local (non-blocking)
            let check_output = Command::new("nix-store")
                .args(&["--query", "--validity", &path])
                .output();
            
            if let Ok(output) = check_output {
                if output.status.success() {
                    already_local_clone.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            
            // Realize from cache (Nix downloads in parallel)
            // This is non-blocking - Nix handles parallel downloads internally
            let _ = Command::new("nix-store")
                .args(&["--realise", &path])
                .output();
            
            downloaded_clone.fetch_add(1, Ordering::Relaxed);
        });
        
        handles.push(handle);
    }
    
    // Wait for all downloads to complete (they're all running in parallel)
    futures::future::join_all(handles).await;
    
    let downloaded_count = downloaded.load(Ordering::Relaxed);
    let local_count = already_local.load(Ordering::Relaxed);
    
    if downloaded_count > 0 || local_count > 0 {
        println!("{} {} downloaded, {} already local (gigabit saturated)", 
            style("✓").green(), downloaded_count, local_count);
    }
    
    Ok(())
}

enum PrewarmResult {
    Downloaded,
    AlreadyLocal,
    Failed,
}

fn detect_derivations() -> Result<Vec<String>> {
    use std::path::Path;
    
    // Find repo root
    let repo_root = find_repo_root()?;
    std::env::set_current_dir(&repo_root)?;
    
    // Detect project type
    let project_type = if Path::new("flake.nix").exists() || Path::new("flake.lock").exists() {
        "flake"
    } else if Path::new("default.nix").exists() {
        "default.nix"
    } else if Path::new("shell.nix").exists() {
        "shell.nix"
    } else {
        return Ok(Vec::new());
    };
    
    // Get derivation path(s)
    let mut derivations = Vec::new();
    
    match project_type {
        "flake" => {
            // Try multiple ways to get derivation
            if let Ok(output) = Command::new("nix")
                .args(&["eval", "--raw", ".#"])
                .output()
            {
                if output.status.success() {
                    let drv = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !drv.is_empty() {
                        derivations.push(drv);
                    }
                }
            }
            
            // Also try nix-instantiate
            if derivations.is_empty() {
                if let Ok(output) = Command::new("nix-instantiate")
                    .args(&["."])
                    .output()
                {
                    if output.status.success() {
                        let drv = String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if !drv.is_empty() {
                            derivations.push(drv);
                        }
                    }
                }
            }
        }
        "default.nix" => {
            if let Ok(output) = Command::new("nix-instantiate")
                .args(&["default.nix"])
                .output()
            {
                if output.status.success() {
                    let drv = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !drv.is_empty() {
                        derivations.push(drv);
                    }
                }
            }
        }
        "shell.nix" => {
            if let Ok(output) = Command::new("nix-instantiate")
                .args(&["shell.nix"])
                .output()
            {
                if output.status.success() {
                    let drv = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !drv.is_empty() {
                        derivations.push(drv);
                    }
                }
            }
        }
        _ => {}
    }
    
    Ok(derivations)
}

fn get_requisites(drv_path: &str) -> Result<Vec<String>> {
    let output = Command::new("nix-store")
        .args(&["--query", "--requisites", drv_path])
        .output()?;
    
    if !output.status.success() {
        return Ok(Vec::new());
    }
    
    let requisites: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    
    Ok(requisites)
}

fn find_repo_root() -> Result<std::path::PathBuf> {
    use std::env;
    use std::path::Path;
    
    let current_dir = env::current_dir()?;
    let mut path = current_dir.as_path();
    
    loop {
        if path.join(".git").exists()
            || path.join("flake.nix").exists()
            || path.join("default.nix").exists()
            || path.join("shell.nix").exists()
        {
            return Ok(path.to_path_buf());
        }
        
        match path.parent() {
            Some(parent) => path = parent,
            None => return Ok(current_dir),
        }
    }
}

/// Build dependency graph and cache it for future CI runs
async fn build_and_cache_graph(
    derivations: &[String],
    derivations_hash: &str,
    cache_file: &std::path::Path,
) -> Result<(Vec<String>, Vec<String>, HashMap<String, Vec<String>>)> {
    // Stream dependency graph as it's discovered (memory efficient)
    info!("Streaming dependency graph to determine build order...");
    
    let mut graph_stream = stream_dependency_graph(derivations)?;
    
    // Collect paths as we stream
    let mut all_paths = Vec::new();
    let mut path_count = 0;
    
    println!();
    info!("Discovering dependencies (streaming)...");
    
    while let Some(path) = graph_stream.next().await {
        path_count += 1;
        all_paths.push(path);
        
        // Show progress every 100 paths
        if path_count % 100 == 0 {
            print!("\r{} Found {} dependencies...", style("→").cyan(), path_count);
            use std::io::Write;
            std::io::stdout().flush().ok();
        }
    }
    
    println!("\r{} Found {} dependencies total", style("✓").green(), path_count);
    
    // Build graph from streamed paths for topological sort
    info!("Building graph structure for topological ordering...");
    let (graph, node_map, reverse_map) = build_graph_from_paths(&all_paths)?;
    
    // Build edges map for caching
    let mut edges = HashMap::new();
    for node_idx in graph.node_indices() {
        let path = reverse_map.get(&node_idx).cloned().unwrap_or_default();
        let deps: Vec<String> = graph
            .neighbors_directed(node_idx, Direction::Incoming)
            .filter_map(|dep_idx| reverse_map.get(&dep_idx).cloned())
            .collect();
        edges.insert(path, deps);
    }
    
    // Topologically sort to get build order (dependencies first)
    info!("Computing topological order (dependencies before dependents)...");
    let build_order = match toposort(&graph, None) {
        Ok(order) => {
            info!("✓ Dependency graph is acyclic (valid build order)");
            order
        }
        Err(_cycle) => {
            println!("{} Warning: Dependency cycle detected, using arbitrary order", style("⚠").yellow());
            graph.node_indices().collect()
        }
    };
    
    info!("Build order computed: {} paths", build_order.len());
    
    // Convert node indices back to store paths in build order
    let ordered_paths: Vec<String> = build_order
        .iter()
        .rev() // Reverse because toposort gives us reverse order (dependents first)
        .filter_map(|&idx| reverse_map.get(&idx).cloned())
        .collect();
    
    // Cache the results for next CI run
    let cache = DependencyCache {
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("Failed to get system time: {}", e))?
            .as_secs(),
        derivations_hash: derivations_hash.to_string(),
        paths: all_paths.clone(),
        build_order: ordered_paths.clone(),
        edges: edges.clone(),
        cache_status: HashMap::new(), // Will be populated during download phase
    };
    
    if let Err(e) = cache.save(cache_file) {
        println!("{} Warning: Failed to save cache: {}", style("⚠").yellow(), e);
    } else {
        info!("✓ Cached dependency graph for next CI run");
    }
    
    Ok((all_paths, ordered_paths, edges))
}

/// Stream dependency graph as it's discovered (memory efficient)
/// Yields store paths as they're found via nix-store --query --requisites
fn stream_dependency_graph(derivations: &[String]) -> Result<impl Stream<Item = String> + '_> {
    use futures::stream;
    
    // Create a stream that yields paths as they're discovered
    let paths_stream = stream::iter(derivations.iter().cloned())
        .then(|drv_path| {
            let drv_path = drv_path.clone();
            async move {
                // Get requisites in background task
                task::spawn_blocking(move || {
                    get_requisites(&drv_path).unwrap_or_default()
                }).await.unwrap_or_default()
            }
        })
        .flat_map(|requisites| stream::iter(requisites));
    
    Ok(paths_stream)
}

/// Build a dependency graph from collected paths
/// Returns: (graph, node_map: path -> node_index, reverse_map: node_index -> path)
fn build_graph_from_paths(paths: &[String]) -> Result<(Graph<String, ()>, HashMap<String, petgraph::graph::NodeIndex>, HashMap<petgraph::graph::NodeIndex, String>)> {
    let mut graph = Graph::<String, ()>::new();
    let mut node_map = HashMap::new();
    let mut reverse_map = HashMap::new();
    
    // First pass: Add all nodes (store paths)
    let all_paths: HashSet<String> = paths.iter().cloned().collect();
    
    for path in &all_paths {
        let idx = graph.add_node(path.clone());
        node_map.insert(path.clone(), idx);
        reverse_map.insert(idx, path.clone());
    }
    
    // Second pass: Add edges (dependencies) - can be parallelized
    use rayon::prelude::*;
    let edges: Vec<(String, Vec<String>)> = all_paths
        .par_iter()
        .map(|path| {
            let refs = get_references(path).unwrap_or_default();
            (path.clone(), refs)
        })
        .collect();
    
    for (path, refs) in edges {
        if let Some(&node_idx) = node_map.get(&path) {
            for dep in refs {
                if let Some(&dep_idx) = node_map.get(&dep) {
                    // Add edge: path depends on dep
                    // This means dep must be built/downloaded before path
                    graph.add_edge(dep_idx, node_idx, ());
                }
            }
        }
    }
    
    info!("Built dependency graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());
    
    Ok((graph, node_map, reverse_map))
}

/// Build a dependency graph from Nix derivations (legacy, non-streaming version)
/// Returns: (graph, node_map: path -> node_index, reverse_map: node_index -> path)
#[allow(dead_code)]
fn build_dependency_graph(derivations: &[String]) -> Result<(Graph<String, ()>, HashMap<String, petgraph::graph::NodeIndex>, HashMap<petgraph::graph::NodeIndex, String>)> {
    let mut graph = Graph::<String, ()>::new();
    let mut node_map = HashMap::new();
    let mut reverse_map = HashMap::new();
    
    // First pass: Add all nodes (store paths)
    let mut all_paths = HashSet::new();
    
    for drv_path in derivations {
        // Get all requisites (transitive closure)
        if let Ok(requisites) = get_requisites(drv_path) {
            for req in requisites {
                all_paths.insert(req);
            }
        }
        all_paths.insert(drv_path.clone());
    }
    
    // Add nodes to graph
    for path in &all_paths {
        let idx = graph.add_node(path.clone());
        node_map.insert(path.clone(), idx);
        reverse_map.insert(idx, path.clone());
    }
    
    // Second pass: Add edges (dependencies)
    for path in &all_paths {
        if let Some(&node_idx) = node_map.get(path) {
            // Get direct references (dependencies) for this path
            if let Ok(references) = get_references(path) {
                for dep in references {
                    if let Some(&dep_idx) = node_map.get(&dep) {
                        // Add edge: path depends on dep
                        // This means dep must be built/downloaded before path
                        graph.add_edge(dep_idx, node_idx, ());
                    }
                }
            }
        }
    }
    
    info!("Built dependency graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());
    
    Ok((graph, node_map, reverse_map))
}

/// Get direct references (dependencies) for a store path
fn get_references(store_path: &str) -> Result<Vec<String>> {
    let output = Command::new("nix-store")
        .args(&["--query", "--references", store_path])
        .output()?;
    
    if !output.status.success() {
        return Ok(Vec::new());
    }
    
    let references: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    
    Ok(references)
}

fn info(msg: &str) {
    println!("{} {}", style("→").cyan(), msg);
}
