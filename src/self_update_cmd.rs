use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::sig_verify;

/// Self-update the flakecache binary from CDN with optional signature verification.
///
/// Downloads the latest (or specified) version from c.flakecache.com/cli and verifies
/// the signature if available. Uses the embedded public key for verification.
///
/// # CDN Layout
///
/// Binary: `https://c.flakecache.com/cli/{version}/{target}/flakecache`
/// Signature: `https://c.flakecache.com/cli/{version}/{target}/flakecache.sig`
///
/// Example: `https://c.flakecache.com/cli/latest/x86_64-unknown-linux-musl/flakecache`
pub fn self_update(tag: Option<&str>) -> Result<()> {
    println!("⬇️  Checking for flakecache updates...");

    // Hard-coded CDN base (Tigris/S3 fronted by c.flakecache.com)
    let base = "https://c.flakecache.com/cli";

    // Target triple detected by self_update (e.g., x86_64-unknown-linux-musl)
    let target = self_update::get_target();

    // Version to fetch: explicit tag or "latest"
    let version = tag.unwrap_or("latest");

    // Layout we expect on the CDN:
    //   {base}/{version}/{target}/flakecache
    // Example: https://c.flakecache.com/cli/latest/x86_64-unknown-linux-musl/flakecache
    let url = format!("{base}/{version}/{target}/flakecache");
    let sig_url = format!("{base}/{version}/{target}/flakecache.sig");

    let current_exe = std::env::current_exe()?;
    download_and_replace_with_signature(&url, &sig_url, &current_exe)?;

    println!("✅ Updated to {version} (target {target})");
    Ok(())
}

/// Download binary and signature, verify signature, then atomically replace current executable.
///
/// # Signature Verification Flow
///
/// 1. Downloads binary from `binary_url`
/// 2. Attempts to download signature from `sig_url` (optional - if 404, skips verification)
/// 3. If signature available: verifies binary against embedded public key
/// 4. If verification passes: atomically replaces current executable
///
/// # Notes
///
/// - Signature verification is optional (fails gracefully if signature 404s)
/// - Uses `sig_verify::verify_signature()` with embedded public key
/// - Atomic replacement ensures no partial updates
fn download_and_replace_with_signature(
    binary_url: &str,
    sig_url: &str,
    current_exe: &PathBuf,
) -> Result<()> {
    // Download binary
    println!("⬇️  Downloading binary from {binary_url}...");
    let resp = reqwest::blocking::get(binary_url)?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "HTTP {} downloading binary from {}",
            resp.status(),
            binary_url
        ));
    }

    let binary_bytes = resp.bytes()?;
    println!("✓ Downloaded {} bytes", binary_bytes.len());

    // Write to temporary file
    let tmp_path = current_exe.with_extension("new");
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(&binary_bytes)?;
    }

    // Download and verify signature (required)
    let sig_resp = reqwest::blocking::get(sig_url)?;
    if !sig_resp.status().is_success() {
        let _ = fs::remove_file(&tmp_path);
        return Err(anyhow::anyhow!(
            "Signature fetch failed (HTTP {}): verification required",
            sig_resp.status()
        ));
    }

    let sig_b64 = sig_resp.text()?;
    println!("🔐 Verifying signature...");
    if let Err(e) = sig_verify::verify_signature(&tmp_path, sig_b64.trim()) {
        let _ = fs::remove_file(&tmp_path);
        return Err(anyhow::anyhow!("Signature verification failed: {e}"));
    }
    println!("✓ Signature verified!");

    // Ensure executable bit on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp_path, perms)?;
    }

    // Atomic replace (on Windows this may fail if file locked; caller should rerun)
    fs::rename(&tmp_path, current_exe)?;
    Ok(())
}

/// Legacy function kept for backwards compatibility (not used in current flow)
#[allow(dead_code)]
fn download_and_replace(url: &str, current_exe: &PathBuf) -> Result<()> {
    let resp = reqwest::blocking::get(url)?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("HTTP {} for {}", resp.status(), url));
    }

    let bytes = resp.bytes()?;

    // Write to temporary file next to the current exe
    let tmp_path = current_exe.with_extension("new");
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(&bytes)?;
    }

    // Ensure executable bit on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp_path, perms)?;
    }

    // Atomic replace (on Windows this may fail if file locked; caller should rerun)
    fs::rename(&tmp_path, current_exe)?;
    Ok(())
}
