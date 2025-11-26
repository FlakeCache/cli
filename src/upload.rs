use crate::auth;
use crate::cbor_client::{CacheInfo, CborClient, NarInfoRequest};
use anyhow::Result;
use console::style;
use crc32fast::Hasher as Crc32Hasher;
use sha2::{Digest, Sha256};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::Semaphore;
use tokio::task;

// Validation helpers
fn validate_token() -> Result<String> {
    print!("  {} Checking authentication token... ", style("→").cyan());
    let token = auth::load_token()?
        .or_else(|| std::env::var("FLAKECACHE_TOKEN").ok())
        .ok_or_else(|| {
            println!("{}", style("✗").red());
            anyhow::anyhow!(
                "No token found. Run 'flakecache login' or set FLAKECACHE_TOKEN env var"
            )
        })?;
    println!("{}", style("✓").green());
    Ok(token)
}

fn validate_nix() -> Result<()> {
    print!("  {} Checking Nix installation... ", style("→").cyan());
    let nix_check = Command::new("nix").args(["--version"]).output();
    match nix_check {
        Ok(output) if output.status.success() => {
            println!("{}", style("✓").green());
            Ok(())
        }
        _ => {
            println!("{}", style("✗").red());
            Err(anyhow::anyhow!(
                "Nix is not installed or not in PATH. Please install Nix first."
            ))
        }
    }
}

async fn validate_cache_access(cache: &str, api_url: &str, token: &str) -> Result<()> {
    print!("  {} Checking cache access... ", style("→").cyan());
    let cbor_client = CborClient::new(api_url, token);

    match cbor_client
        .get::<CacheInfo>(&format!("/caches/{cache}"))
        .await
    {
        Ok(_) => {
            println!("{}", style("✓").green());
            Ok(())
        }
        Err(e) => {
            println!("{}", style("✗").red());
            let error_msg = e.to_string();
            if error_msg.contains("404") {
                Err(anyhow::anyhow!(
                    "Cache '{cache}' not found. Please create it first at https://flakecache.com/caches"
                ))
            } else if error_msg.contains("403") {
                Err(anyhow::anyhow!(
                    "Access denied to cache '{cache}'. Check your token permissions."
                ))
            } else {
                Err(anyhow::anyhow!("Failed to check cache: {error_msg}"))
            }
        }
    }
}

fn get_store_paths(store_paths: Option<Vec<String>>) -> Result<Vec<String>> {
    if let Some(paths) = store_paths {
        return Ok(paths);
    }

    // Get from nix build
    let output = Command::new("nix").args(["build", "--json"]).output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("nix build failed. Run 'nix build' first."));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut paths = Vec::new();

    if let Some(array) = json.as_array() {
        for item in array {
            if let Some(outputs) = item.get("outputs") {
                if let Some(outputs_obj) = outputs.as_object() {
                    for (_, value) in outputs_obj {
                        if let Some(path) = value.as_str() {
                            paths.push(path.to_string());
                        }
                    }
                }
            }
        }
    }

    if paths.is_empty() {
        return Err(anyhow::anyhow!(
            "No build outputs found. Run 'nix build' first."
        ));
    }

    Ok(paths)
}

// Compression and hashing helpers
struct CompressionResult {
    file_hash: String,
    crc32_checksum: u32,
    total_size: u64,
    final_file: std::path::PathBuf,
}

async fn compress_and_hash_nar(nar_data: Vec<u8>) -> Result<CompressionResult> {
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("flakecache-temp-{}.nar.xz", std::process::id()));

    // Spawn xz process
    let mut xz_cmd = TokioCommand::new("xz")
        .args(["-c"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    // Write NAR to xz stdin in background
    let mut xz_stdin = xz_cmd
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to open xz stdin"))?;
    drop(tokio::spawn(async move {
        let _ = xz_stdin.write_all(&nar_data).await;
        let _ = xz_stdin.shutdown().await;
    }));

    // Stream xz output to disk while calculating hash and CRC32
    let mut file = TokioFile::create(&temp_file).await?;
    let mut sha256_hasher = Sha256::new();
    let mut crc_hasher = Crc32Hasher::new();
    let xz_stdout = xz_cmd
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to open xz stdout"))?;
    let mut reader = BufReader::new(xz_stdout);
    let mut buffer = vec![0u8; 8192];
    let mut total_size = 0u64;

    // Stream from xz to disk, hashing as we go
    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }

        let chunk = &buffer[..bytes_read];
        file.write_all(chunk).await?;
        sha256_hasher.update(chunk);
        crc_hasher.update(chunk);
        total_size += bytes_read as u64;
    }

    file.sync_all().await?;

    // Wait for xz to finish
    let exit_status = xz_cmd.wait().await?;
    if !exit_status.success() {
        return Err(anyhow::anyhow!(
            "xz compression failed with exit code: {exit_status}"
        ));
    }

    // Calculate final hash and CRC32
    let hash_bytes = sha256_hasher.finalize();
    let file_hash =
        base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &hash_bytes).to_lowercase();
    let crc32_checksum = crc_hasher.finalize();

    // Rename temp file to final name
    let final_file = temp_dir.join(format!("flakecache-{file_hash}.nar.xz"));
    std::fs::rename(&temp_file, &final_file)?;

    Ok(CompressionResult {
        file_hash,
        crc32_checksum,
        total_size,
        final_file,
    })
}

// Upload helpers
async fn upload_nar(
    cbor_client: &CborClient,
    cache: &str,
    file_hash: &str,
    final_file: &std::path::PathBuf,
) -> Result<()> {
    let mut file = TokioFile::open(final_file).await?;
    let mut nar_data = Vec::new();
    let _ = file.read_to_end(&mut nar_data).await?;

    let nar_path = format!("/{cache}/nar/{file_hash}/xz");
    cbor_client.put_binary(&nar_path, nar_data).await?;
    Ok(())
}

fn get_references(store_path: &str) -> Vec<String> {
    let references_output = Command::new("nix-store")
        .args(["--query", "--references", store_path])
        .output();

    if let Ok(output) = references_output {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| {
                    line.split('/')
                        .next_back()
                        .unwrap_or("")
                        .split('-')
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

struct NarInfoData<'a> {
    cbor_client: &'a CborClient,
    cache: &'a str,
    store_path: &'a str,
    nar_hash: &'a str,
    file_hash: &'a str,
    file_size: u64,
    nar_size: usize,
    references: Vec<String>,
}

async fn upload_narinfo(data: &NarInfoData<'_>) -> Result<()> {
    let narinfo_content = format!(
        "StorePath: {}\nURL: nar/{}.nar.xz\nCompression: xz\nFileHash: sha256:{}\nFileSize: {}\nNarHash: sha256:{}\nNarSize: {}\nReferences: {}\n",
        data.store_path, data.file_hash, data.file_hash, data.file_size, data.nar_hash, data.nar_size, data.references.join(" ")
    );

    let narinfo_request = NarInfoRequest {
        narinfo: narinfo_content,
    };
    let narinfo_path = format!("/{}/{}", data.cache, data.nar_hash);
    data.cbor_client
        .put_cbor(&narinfo_path, &narinfo_request)
        .await?;
    Ok(())
}

async fn upload_store_path(cbor_client: &CborClient, cache: &str, store_path: &str) -> Result<()> {
    println!("Uploading {store_path}...");

    // Build NAR
    let nar_output = Command::new("nix-store")
        .args(["--dump", store_path])
        .output()?;

    if !nar_output.status.success() {
        return Err(anyhow::anyhow!("Failed to build NAR for {store_path}"));
    }

    // Calculate NAR hash (uncompressed)
    let nar_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&nar_output.stdout);
        let hash_bytes = hasher.finalize();
        base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &hash_bytes).to_lowercase()
    };

    let nar_size = nar_output.stdout.len();

    // Compress and hash
    let compression_result = compress_and_hash_nar(nar_output.stdout).await?;

    // Get file size for NARInfo (before upload/cleanup)
    let file_size = std::fs::metadata(&compression_result.final_file)
        .map(|m| m.len())
        .unwrap_or(compression_result.total_size);

    // Upload NAR
    if let Err(e) = upload_nar(
        cbor_client,
        cache,
        &compression_result.file_hash,
        &compression_result.final_file,
    )
    .await
    {
        eprintln!("Failed to upload NAR: {e}");
        let _ = std::fs::remove_file(&compression_result.final_file);
        return Err(e);
    }

    println!(
        "  ✓ NAR uploaded ({} bytes compressed, CRC32: {:08x})",
        compression_result.total_size, compression_result.crc32_checksum
    );

    // Clean up temp file
    let _ = std::fs::remove_file(&compression_result.final_file);

    // Get references and upload NARInfo
    let references = get_references(store_path);
    let narinfo_data = NarInfoData {
        cbor_client,
        cache,
        store_path,
        nar_hash: &nar_hash,
        file_hash: &compression_result.file_hash,
        file_size,
        nar_size,
        references,
    };
    upload_narinfo(&narinfo_data).await?;

    println!("  ✓ NARInfo uploaded");
    Ok(())
}

#[allow(clippy::too_many_lines)] // Main upload function coordinates multiple operations
pub async fn upload(cache: &str, store_paths: Option<Vec<String>>, api_url: &str) -> Result<()> {
    println!(
        "{}",
        style("=== Uploading to FlakeCache ===\n").bold().cyan()
    );

    // Validation checks
    println!("{} Running validation checks...", style("✓").green());
    let token = validate_token()?;
    validate_nix()?;
    validate_cache_access(cache, api_url, &token).await?;

    // Get store paths
    print!("  {} Finding store paths to upload... ", style("→").cyan());
    let paths = get_store_paths(store_paths)?;
    println!("{} Found {} path(s)", style("✓").green(), paths.len());

    println!();
    println!(
        "Uploading {} store path(s) to cache: {}\n",
        paths.len(),
        cache
    );

    // Use CBOR client for fast binary API
    let cbor_client = CborClient::new(api_url, &token);

    for store_path in paths {
        if let Err(e) = upload_store_path(&cbor_client, cache, &store_path).await {
            eprintln!("Failed to upload {store_path}: {e}");
            // Continue with next path
        }
    }

    println!("\n{} Upload complete!", style("✓").green());
    Ok(())
}

/// Warm cache by building and uploading store paths
pub async fn warm(
    cache: &str,
    paths: Option<String>,
    flake: Option<String>,
    expression: Option<String>,
    api_url: &str,
) -> Result<()> {
    println!("{}", style("=== Warming FlakeCache ===\n").bold().cyan());

    // Get token from config or env (validated but not used yet - reserved for future auth checks)
    let _token = auth::load_token()?
        .or_else(|| std::env::var("FLAKECACHE_TOKEN").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No token found. Run 'flakecache login' or set FLAKECACHE_TOKEN env var"
            )
        })?;

    // Resolve store paths to build
    let store_paths = if let Some(paths_str) = paths {
        // Parse comma-separated paths
        let path_list: Vec<String> = paths_str.split(',').map(|s| s.trim().to_string()).collect();
        build_paths(&path_list)?
    } else if let Some(flake_ref) = flake {
        if let Some(expr) = expression {
            build_from_flake(&flake_ref, &expr)?
        } else {
            return Err(anyhow::anyhow!("--expression required when using --flake"));
        }
    } else if let Some(expr) = expression {
        build_from_expression(&expr)?
    } else {
        return Err(anyhow::anyhow!(
            "Must specify --paths, --flake with --expression, or --expression"
        ));
    };

    println!(
        "Building and uploading {} store path(s) to cache: {}\n",
        store_paths.len(),
        cache
    );

    // Upload each store path
    upload(cache, Some(store_paths), api_url).await?;

    println!("\n{} Cache warmed successfully!", style("✓").green());

    Ok(())
}

fn build_paths(paths: &[String]) -> Result<Vec<String>> {
    let mut store_paths = Vec::new();

    for path in paths {
        if path.starts_with("/nix/store/") {
            // Already a store path
            store_paths.push(path.clone());
        } else {
            // Try to build it as a package
            let output = Command::new("nix-build")
                .args(["-E", &format!("with import <nixpkgs> {{}}; {path}")])
                .output()?;

            if output.status.success() {
                let store_path = String::from_utf8(output.stdout)?.trim().to_string();
                store_paths.push(store_path);
            } else {
                return Err(anyhow::anyhow!(
                    "Failed to build {}: {}",
                    path,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }

    Ok(store_paths)
}

fn build_from_flake(flake_ref: &str, expression: &str) -> Result<Vec<String>> {
    let output = Command::new("nix")
        .args(["build", &format!("{flake_ref}#{expression}"), "--json"])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "nix build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    extract_store_paths(&json)
}

fn build_from_expression(expr: &str) -> Result<Vec<String>> {
    let output = Command::new("nix")
        .args(["build", "-E", expr, "--json"])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "nix build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    extract_store_paths(&json)
}

fn extract_store_paths(json: &serde_json::Value) -> Result<Vec<String>> {
    let mut paths = Vec::new();

    match json {
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(outputs) = item.get("outputs") {
                    if let Some(outputs_obj) = outputs.as_object() {
                        for (_, value) in outputs_obj {
                            if let Some(path) = value.as_str() {
                                paths.push(path.to_string());
                            }
                        }
                    }
                } else if let Some(path) = item.as_str() {
                    paths.push(path.to_string());
                }
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(outputs) = obj.get("outputs") {
                if let Some(outputs_obj) = outputs.as_object() {
                    for (_, value) in outputs_obj {
                        if let Some(path) = value.as_str() {
                            paths.push(path.to_string());
                        }
                    }
                }
            }
        }
        serde_json::Value::String(path) => {
            paths.push(path.clone());
        }
        _ => {}
    }

    if paths.is_empty() {
        return Err(anyhow::anyhow!("No store paths found in build output"));
    }

    Ok(paths)
}

/// Pre-warm cache by downloading all dependencies from cache
/// Find repository root by walking up directories looking for .git, flake.nix, or default.nix
fn find_repo_root() -> Result<std::path::PathBuf> {
    use std::env;

    let current_dir = env::current_dir()?;
    let mut path = current_dir.as_path();

    loop {
        // Check for repository markers
        if path.join(".git").exists()
            || path.join("flake.nix").exists()
            || path.join("default.nix").exists()
            || path.join("shell.nix").exists()
        {
            return Ok(path.to_path_buf());
        }

        // Move to parent directory
        match path.parent() {
            Some(parent) => path = parent,
            None => {
                // Reached filesystem root, return current directory as fallback
                return Ok(current_dir);
            }
        }
    }
}

/// Auto-detects project type and downloads all requisites
pub async fn prewarm() -> Result<()> {
    use console::style;
    use std::env;
    use std::path::Path;

    println!(
        "{}",
        style("=== Pre-warming FlakeCache ===\n").bold().cyan()
    );

    // Validation checks
    println!("{} Running validation checks...", style("✓").green());

    // Check 1: Nix availability
    print!("  {} Checking Nix installation... ", style("→").cyan());
    let nix_check = Command::new("nix").args(["--version"]).output();
    match nix_check {
        Ok(output) if output.status.success() => {
            println!("{}", style("✓").green());
        }
        _ => {
            println!("{}", style("✗").red());
            return Err(anyhow::anyhow!(
                "Nix is not installed or not in PATH. Please install Nix first."
            ));
        }
    }

    // Check 2: Find repository root
    print!("  {} Detecting repository root... ", style("→").cyan());
    let repo_root = find_repo_root()?;
    println!("{} {}", style("✓").green(), repo_root.display());

    // Change to repository root for all Nix commands
    env::set_current_dir(&repo_root)?;

    // Check 3: Project type detection
    print!("  {} Detecting project type... ", style("→").cyan());
    let project_type = if Path::new("flake.nix").exists() || Path::new("flake.lock").exists() {
        Some("flake")
    } else if Path::new("default.nix").exists() {
        Some("default.nix")
    } else if Path::new("shell.nix").exists() {
        Some("shell.nix")
    } else {
        None
    };

    match project_type {
        Some("flake") => println!("{} Flake project", style("✓").green()),
        Some("default.nix") => println!("{} default.nix project", style("✓").green()),
        Some("shell.nix") => println!("{} shell.nix project", style("✓").green()),
        Some(_) => println!("{} Unknown project type", style("?").yellow()),
        None => {
            println!("{}", style("⚠").yellow());
            println!("   No flake.nix, default.nix, or shell.nix found");
            println!("   Nix will still check the cache automatically during builds");
            return Ok(());
        }
    }

    println!();
    println!("Downloading all dependencies from cache...\n");

    match project_type {
        Some("flake") => {
            // Get derivation path(s)
            let drv_output = Command::new("nix")
                .args(["eval", "--raw", ".#"])
                .output()
                .or_else(|_| Command::new("nix-instantiate").args(["."]).output())?;

            if drv_output.status.success() {
                let drv_path = String::from_utf8(drv_output.stdout)?
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !drv_path.is_empty() {
                    prewarm_derivation(&drv_path).await?;
                }
            }

            // Also realize the main derivation
            println!("Realizing main derivation...");
            let _ = Command::new("nix")
                .args(["build", "--no-link", "."])
                .output();
        }
        Some("default.nix") => {
            let drv_output = Command::new("nix-instantiate")
                .args(["default.nix"])
                .output()?;

            if drv_output.status.success() {
                let drv_path = String::from_utf8(drv_output.stdout)?
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !drv_path.is_empty() {
                    prewarm_derivation(&drv_path).await?;
                }
            }

            // Realize the main derivation
            let _ = Command::new("nix-build")
                .args(["--no-out-link", "default.nix"])
                .output();
        }
        Some("shell.nix") => {
            let drv_output = Command::new("nix-instantiate")
                .args(["shell.nix"])
                .output()?;

            if drv_output.status.success() {
                let drv_path = String::from_utf8(drv_output.stdout)?
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !drv_path.is_empty() {
                    prewarm_derivation(&drv_path).await?;
                }
            }
        }
        Some(_) | None => {
            // Unknown project type or unreachable (we return early in validation checks)
        }
    }

    println!(
        "\n{} Pre-warm complete - all available dependencies downloaded from cache",
        style("✓").green()
    );

    Ok(())
}

async fn prewarm_derivation(drv_path: &str) -> Result<()> {
    use console::style;
    use std::io::{BufRead, BufReader};
    use std::process::{Command as StdCommand, Stdio};
    use tokio::io::AsyncBufReadExt;
    use tokio::process::Command;

    // Query all requisites
    let query_output = StdCommand::new("nix-store")
        .args(["--query", "--requisites", drv_path])
        .output()?;

    if !query_output.status.success() {
        return Ok(()); // Silently fail if query doesn't work
    }

    let requisites: Vec<String> = BufReader::new(query_output.stdout.as_slice())
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .collect();

    if requisites.is_empty() {
        return Ok(());
    }

    println!(
        "Found {} store paths that will be downloaded from cache:",
        requisites.len()
    );
    println!();

    // Show list of what will be downloaded (first 20, then summary)
    let display_count = requisites.len().min(20);
    for (i, path) in requisites.iter().take(display_count).enumerate() {
        // Extract package name from store path (e.g., /nix/store/abc123-package-name-1.0)
        let name = path
            .split('/')
            .next_back()
            .unwrap_or("")
            .split('-')
            .skip(1)
            .take(3)
            .collect::<Vec<_>>()
            .join("-");

        if name.is_empty() {
            println!("  {}. {}", i + 1, path);
        } else {
            println!("  {}. {}", i + 1, name);
        }
    }

    if requisites.len() > display_count {
        println!("  ... and {} more", requisites.len() - display_count);
    }

    println!();
    println!(
        "{} Downloading from cache (streaming progress)...",
        style("⬇️").cyan()
    );
    println!();

    // Realize all dependencies in parallel (4 at a time) with streaming output
    let semaphore = Arc::new(Semaphore::new(4));
    let mut handles = Vec::new();

    for path in requisites {
        let sem = semaphore.clone();
        let path_clone = path.clone();
        let handle = task::spawn(async move {
            let Ok(_permit) = sem.acquire().await else {
                return None;
            };

            // Stream output from nix-store --realise
            let Ok(mut child) = Command::new("nix-store")
                .args(["--realise", &path_clone])
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
            else {
                return None;
            };

            let mut downloaded = false;

            // Stream stderr (where nix-store outputs progress) in real-time
            if let Some(mut stderr) = child.stderr.take() {
                let mut reader = tokio::io::BufReader::new(&mut stderr);
                let mut line = String::new();

                // Read chunks and process line by line
                loop {
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break, // EOF or error
                        Ok(_) => {
                            let trimmed = line.trim();

                            // Nix signals downloads via stderr with these messages:
                            // - "querying info on" - checking if package exists (narinfo request)
                            // - "downloading" - actively downloading NAR file
                            // - "substituting" - found in cache, using it
                            // - "copying" - copying from local store

                            // Extract package name for cleaner output
                            let pkg_name = path_clone
                                .split('/')
                                .next_back()
                                .unwrap_or("")
                                .split('-')
                                .skip(1)
                                .take(2)
                                .collect::<Vec<_>>()
                                .join("-");

                            if trimmed.contains("querying info on") || trimmed.contains("querying")
                            {
                                // Nix is checking if package exists (about to download)
                                println!("  🔍 Checking {pkg_name}...");
                            } else if trimmed.contains("downloading") {
                                // Nix is actively downloading
                                // Nix output: "downloading '...' (123.45 MiB)" or "downloading '...' [123.45/456.78 MiB]"
                                let size_info = trimmed
                                    .find('(')
                                    .and_then(|start| {
                                        trimmed.find(')').map(|end| &trimmed[start + 1..end])
                                    })
                                    .unwrap_or("");

                                if size_info.is_empty() {
                                    println!("  ⬇️  Downloading {pkg_name}");
                                } else {
                                    println!("  ⬇️  Downloading {pkg_name} {size_info}");
                                }
                                downloaded = true;
                            } else if trimmed.contains("substituting") {
                                // Found in cache, using it (no download needed)
                                println!("  ✓ {pkg_name} (from cache)");
                                downloaded = true;
                            } else if trimmed.contains("copying") {
                                // Copying from local store
                                println!("  📦 Copying {pkg_name}...");
                                downloaded = true;
                            }

                            line.clear();
                        }
                    }
                }
            }

            // Wait for process to complete
            let _ = child.wait().await;

            if downloaded {
                Some(path_clone)
            } else {
                None
            }
        });
        handles.push(handle);
    }

    // Wait for all and count downloads
    let mut downloaded = 0;
    for handle in handles {
        if let Ok(Some(_)) = handle.await {
            downloaded += 1;
        }
    }

    println!();
    if downloaded > 0 {
        println!(
            "{} Downloaded {} paths from cache",
            style("✓").green(),
            downloaded
        );
    } else {
        println!("{} All paths already available locally", style("✓").green());
    }

    Ok(())
}
