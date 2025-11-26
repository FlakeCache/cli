//! Ed25519 signature verification for CLI binary integrity.
//!
//! This module provides signature verification for `FlakeCache` CLI binaries using
//! embedded Ed25519 public key. Each release binary can be verified against a
//! detached `.sig` file published on the CDN.
//!
//! # Model
//!
//! The CLI embeds the public key at compile time. Users can verify downloaded
//! binaries against detached signatures:
//!
//! ```bash
//! # Download binary and signature
//! curl -o flakecache https://c.flakecache.com/cli/latest/x86_64-unknown-linux-musl/flakecache
//! curl -o flakecache.sig <https://c.flakecache.com/cli/latest/x86_64-unknown-linux-musl/flakecache.sig>
//!
//! # Verify using embedded public key (built into CLI)
//! flakecache verify-self
//! ```
//!
//! The signature file contains a base64-encoded Ed25519 signature of the binary bytes.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use std::fs;
use std::path::Path;

/// Embedded Ed25519 public key (base64-encoded). Override at runtime with
/// `FLAKECACHE_CLI_PUBKEY_B64` for rotations.
const EMBEDDED_PUBLIC_KEY_B64: &str =
    "MCowBQYDK2VwAyEAL2OKkHEtUlDwc4YwQwl3sgj2bcwCwTK7DhfE4nyc4z4=";

/// Verify a binary file against a detached Ed25519 signature.
///
/// # Arguments
/// * `binary_path` - Path to the binary file to verify
/// * `signature_b64` - Base64-encoded Ed25519 signature
///
/// # Returns
/// * `Ok(())` if signature is valid
/// * `Err` if signature is invalid or file cannot be read
pub fn verify_signature(binary_path: &Path, signature_b64: &str) -> Result<()> {
    // Public key: allow env override for key rotations
    let pubkey_b64 = std::env::var("FLAKECACHE_CLI_PUBKEY_B64")
        .unwrap_or_else(|_| EMBEDDED_PUBLIC_KEY_B64.to_string());

    // Decode public key from base64
    let public_key_bytes = BASE64
        .decode(pubkey_b64)
        .context("Failed to decode embedded public key")?;

    if public_key_bytes.len() != PUBLIC_KEY_LENGTH {
        return Err(anyhow!(
            "Invalid embedded public key length: {} (expected {})",
            public_key_bytes.len(),
            PUBLIC_KEY_LENGTH
        ));
    }

    // Convert bytes to VerifyingKey
    let verifying_key = VerifyingKey::from_bytes(
        &public_key_bytes[..PUBLIC_KEY_LENGTH]
            .try_into()
            .context("Failed to convert public key bytes")?,
    )
    .context("Invalid embedded public key format")?;

    // Decode signature from base64
    let signature_bytes = BASE64
        .decode(signature_b64)
        .context("Failed to decode signature (not valid base64)")?;

    if signature_bytes.len() != SIGNATURE_LENGTH {
        return Err(anyhow!(
            "Invalid signature length: {} (expected {})",
            signature_bytes.len(),
            SIGNATURE_LENGTH
        ));
    }

    let signature = Signature::from_bytes(
        &signature_bytes[..SIGNATURE_LENGTH]
            .try_into()
            .context("Failed to convert signature bytes")?,
    );

    // Read binary file
    let binary_bytes = fs::read(binary_path)
        .with_context(|| format!("Failed to read binary file: {}", binary_path.display()))?;

    // Verify signature
    verifying_key
        .verify(&binary_bytes, &signature)
        .context("Signature verification failed")?;

    Ok(())
}

/// Verify the current executable against a detached signature file.
///
/// # Arguments
/// * `signature_path` - Path to the detached `.sig` file (base64-encoded)
///
/// # Returns
/// * `Ok(())` if signature is valid
/// * `Err` if verification fails
pub fn verify_self(signature_path: &Path) -> Result<()> {
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;

    // Read signature file
    let signature_b64 = fs::read_to_string(signature_path).with_context(|| {
        format!(
            "Failed to read signature file: {}",
            signature_path.display()
        )
    })?;

    verify_signature(&current_exe, signature_b64.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_signature_base64() {
        let result = verify_signature(Path::new("/tmp/fake-binary"), "not-valid-base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_binary_path() {
        let result = verify_signature(
            Path::new("/nonexistent/path/binary"),
            "aGVsbG8K", // valid base64 but wrong length
        );
        assert!(result.is_err());
    }
}
