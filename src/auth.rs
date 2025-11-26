use anyhow::Result;
use base64::{self, engine::general_purpose::URL_SAFE as B64_URL_SAFE, Engine};
use console::style;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::{Timestamp, Uuid};

#[derive(Debug, Serialize, Deserialize)]
struct AuthConfig {
    token: String,
    api_url: String,
    expires_at: Option<i64>, // Unix timestamp in seconds
}

/// JWT claims (only includes fields we care about)
#[derive(Debug, Deserialize)]
struct JwtClaims {
    /// Expiration time (seconds since Unix epoch)
    #[serde(default)]
    exp: Option<i64>,
}

/// Clock skew tolerance for token expiry checks (in seconds)
/// Allows for minor time differences between client and server
const CLOCK_SKEW_TOLERANCE: i64 = 60;

pub fn get_config_path() -> Result<PathBuf> {
    // Use cache directory (~/.cache/flakecache/auth.json) instead of config directory
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find cache directory"))?
        .join("flakecache");

    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir.join("auth.json"))
}

/// Parse JWT expiry claim from a token (gracefully handles non-JWT tokens)
/// Returns the expiration timestamp in seconds since Unix epoch, or None if not a JWT or no exp claim
fn parse_jwt_expiry(token: &str) -> Option<i64> {
    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        // Not a JWT, return None
        return None;
    }

    let payload = parts[1];

    // Decode base64url payload
    // Base64url has different padding rules - we need to add padding if missing
    let mut padded = payload.to_string();
    match padded.len() % 4 {
        2 => padded.push_str("=="),
        3 => padded.push('='),
        _ => {}
    }

    // Decode from base64url (- and _ are used instead of + and /)
    let Ok(decoded) = B64_URL_SAFE.decode(&padded) else {
        return None;
    };

    // Parse JSON claims
    let Ok(claims) = serde_json::from_slice::<JwtClaims>(&decoded) else {
        return None;
    };

    claims.exp
}

/// Check if a token is expired, accounting for clock skew
#[allow(clippy::cast_possible_wrap)] // System time in seconds won't overflow i64 for centuries
fn is_token_expired(expires_at: i64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Token is expired if: now > (exp + clock_skew)
    // This allows for minor time differences between client and server
    now > expires_at + CLOCK_SKEW_TOLERANCE
}

pub fn load_token() -> Result<Option<String>> {
    // Check environment variable first (highest priority)
    if let Ok(token) = std::env::var("FLAKECACHE_TOKEN") {
        if !token.is_empty() {
            // Check if env var token is expired (if it's a JWT with exp)
            if let Some(exp) = parse_jwt_expiry(&token) {
                if is_token_expired(exp) {
                    return Err(anyhow::anyhow!(
                        "FLAKECACHE_TOKEN has expired. Run 'flakecache login' to refresh."
                    ));
                }
            }
            return Ok(Some(token));
        }
    }

    // Fall back to saved token in cache directory
    let config_path = get_config_path()?;

    if !config_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&config_path)?;
    let config: AuthConfig = serde_json::from_str(&content)?;

    // Check if token is expired
    if let Some(expires_at) = config.expires_at {
        if is_token_expired(expires_at) {
            return Err(anyhow::anyhow!(
                "Token has expired. Run 'flakecache login' to refresh."
            ));
        }
    }

    Ok(Some(config.token))
}

pub fn save_token(token: String, api_url: String) -> Result<()> {
    let config_path = get_config_path()?;

    // Try to extract expiry from JWT token (gracefully handles non-JWT tokens)
    let expires_at = parse_jwt_expiry(&token);

    let config = AuthConfig {
        token,
        api_url,
        expires_at,
    };

    let content = serde_json::to_string_pretty(&config)?;
    fs::write(&config_path, content)?;

    println!("✓ Token saved to: {}", config_path.display());
    if let Some(exp) = expires_at {
        #[allow(clippy::cast_possible_wrap)] // System time in seconds won't overflow i64
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let expires_in_secs = exp - now;

        if expires_in_secs > 0 {
            let hours = expires_in_secs / 3600;
            let days = hours / 24;

            if days > 0 {
                println!("  (Token expires in {days} days)");
            } else if hours > 0 {
                println!("  (Token expires in {hours} hours)");
            } else {
                println!("  (Token expires in {expires_in_secs} seconds)");
            }
        }
    }
    println!("  (You can also set FLAKECACHE_TOKEN environment variable instead)");
    Ok(())
}

/// Login to `FlakeCache` - supports web-based OAuth (like Claude Code) or direct token input
pub async fn login(api_url: &str, token: Option<&str>, force_new_login: bool) -> Result<()> {
    // Check if token is already set via environment variable (unless force_new_login)
    if !force_new_login {
        if let Ok(env_token) = std::env::var("FLAKECACHE_TOKEN") {
            if !env_token.is_empty() && token.is_none() {
                println!("FLAKECACHE_TOKEN already set. Logged in.");
                println!("Use --force-new-login to login with a different account.");
                return Ok(());
            }
        }
    }

    if let Some(token_str) = token {
        // Token provided as argument
        if token_str.is_empty() {
            return Err(anyhow::anyhow!("Token cannot be empty"));
        }

        save_token(token_str.to_string(), api_url.to_string())?;
        return Ok(());
    }

    // Web-based OAuth flow (like GitHub CLI)
    println!("FlakeCache login");

    // Generate OAuth state for security (v7 is time-ordered)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("Failed to get system time: {e}"))?;
    let ts = Timestamp::from_unix(uuid::NoContext, now.as_secs(), now.subsec_nanos());
    let state = Uuid::new_v7(ts).to_string();

    // Start local server first to get the callback URL
    let (callback_url, callback_handle) = start_oauth_callback_server(state.clone()).await?;

    // Build OAuth URL (web-based, no provider selection - server handles it)
    // The server will show a login page where user can choose provider
    // Always use api.flakecache.com for authentication (not c.flakecache.com)
    let base_url = if api_url.contains("c.flakecache.com") {
        "https://api.flakecache.com".to_string()
    } else {
        api_url.replace("/api/v1", "").replace("/api", "")
    };
    // URL encode the callback URL
    let encoded_callback = urlencoding::encode(&callback_url);
    let oauth_url = format!("{base_url}/auth/cli?state={state}&redirect_uri={encoded_callback}");

    // GitHub CLI-style output
    println!(
        "{} Tip: you can generate a personal access token instead:",
        style("!").yellow()
    );
    println!(
        "   {}",
        style("https://api.flakecache.com/settings/tokens").dim()
    );
    println!();
    println!(
        "{} Press Enter to open flakecache.com in your browser...",
        style("→").cyan()
    );

    // Wait for user to press Enter (like GitHub CLI)
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);

    // Open browser (like GitHub CLI)
    if let Err(e) = open::that(&oauth_url) {
        println!();
        println!(
            "{} Could not open browser automatically: {}",
            style("✗").red(),
            e
        );
        println!();
        println!("Please open this URL in your browser:");
        println!("{}", style(&oauth_url).bold());
        println!();
    } else {
        println!(
            "{} Opening {} in your browser...",
            style("→").cyan(),
            base_url
        );
    }

    println!();
    println!("{} Waiting for authentication...", style("→").cyan());

    // Wait for callback
    let token = callback_handle.await??;

    save_token(token, api_url.to_string())?;
    println!();
    println!(
        "{} Authentication complete. Press Enter to continue...",
        style("✓").green()
    );
    let _ = io::stdin().read_line(&mut input);

    Ok(())
}

type TokenResult = Result<String, anyhow::Error>;

/// Start OAuth callback server
/// Returns (`callback_url`, handle) where handle resolves to the token
#[allow(clippy::unused_async)] // Async signature required for API consistency
async fn start_oauth_callback_server(
    state: String,
) -> Result<(String, tokio::task::JoinHandle<TokenResult>)> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    // Start local server on random port
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let callback_url = format!("http://127.0.0.1:{port}");

    // Don't show the callback URL to user (GitHub CLI style - cleaner output)

    // Set timeout (5 minutes)
    let timeout = Duration::from_secs(300);

    listener.set_nonblocking(true)?;

    // Spawn task to handle callback
    let handle = tokio::spawn(async move {
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(anyhow::anyhow!("OAuth timeout - please try again"));
            }

            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0; 4096];
                    if let Ok(size) = stream.read(&mut buffer) {
                        let request = String::from_utf8_lossy(&buffer[..size]);

                        // Parse token from callback
                        if let Some(token) = extract_token_from_request(&request, &state) {
                            // Send success response
                            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Login successful!</h1><p>You can close this window.</p></body></html>";
                            let _ = stream.write_all(response.as_bytes());
                            return Ok(token);
                        }
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No connection yet, wait a bit
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Error accepting connection: {e}"));
                }
            }
        }
    });

    Ok((callback_url, handle))
}

/// Extract token from OAuth callback request
fn extract_token_from_request(request: &str, expected_state: &str) -> Option<String> {
    // Parse query parameters from GET request
    // Expected format: GET /callback?state=...&token=... HTTP/1.1
    if let Some(query_start) = request.find('?') {
        if let Some(query_end) = request[query_start..].find(' ') {
            let query = &request[query_start + 1..query_start + query_end];

            // Parse query params
            let mut state_found = false;
            let mut token = None;

            for param in query.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    match key {
                        "state" if value == expected_state => {
                            state_found = true;
                        }
                        "token" => {
                            token = Some(value.to_string());
                        }
                        _ => {}
                    }
                }
            }

            if state_found {
                return token;
            }
        }
    }

    None
}

/// Show current logged-in user
pub async fn whoami(api_url: &str) -> Result<()> {
    println!("{}", style("=== FlakeCache User ===\n").bold().cyan());

    // Get token from config or env
    let token = load_token()?.ok_or_else(|| {
        anyhow::anyhow!("No token found. Run 'flakecache login' or set FLAKECACHE_TOKEN env var")
    })?;

    // Always use api.flakecache.com for user info (not c.flakecache.com)
    let auth_api_url = if api_url.contains("c.flakecache.com") {
        "https://api.flakecache.com"
    } else {
        api_url
    };

    // Use JSON endpoint for user info (auth API)
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{auth_api_url}/api/v1/user/me"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get user info: {}",
            response.status()
        ));
    }

    let user: serde_json::Value = response.json().await?;

    println!(
        "{} {}",
        style("Email:").bold(),
        user["email"].as_str().unwrap_or("unknown")
    );
    println!(
        "{} {}",
        style("ID:").bold(),
        user["id"].as_u64().unwrap_or(0)
    );
    if let Some(org) = user["organization"].as_str() {
        println!("{} {}", style("Organization:").bold(), org);
    }
    if let Some(plan) = user["plan"].as_str() {
        println!("{} {}", style("Plan:").bold(), plan);
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::cast_possible_wrap)]
mod tests {
    use super::*;

    /// Create a minimal JWT token for testing (header.payload.signature)
    /// The payload contains: {"exp": <`exp_time`>}
    fn create_test_jwt_with_exp(exp: i64) -> String {
        use base64::Engine;

        // Header: {"typ":"JWT","alg":"HS256"}
        let header = B64_URL_SAFE.encode(r#"{"typ":"JWT","alg":"HS256"}"#);

        // Payload: {"exp":<exp_time>}
        let payload = B64_URL_SAFE.encode(format!(r#"{{"exp":{exp}}}"#));

        // Signature (dummy, just needs to be valid base64)
        let signature = B64_URL_SAFE.encode("dummy_signature");

        format!("{header}.{payload}.{signature}")
    }

    /// Create a JWT token without exp claim
    fn create_test_jwt_without_exp() -> String {
        use base64::Engine;

        let header = B64_URL_SAFE.encode(r#"{"typ":"JWT","alg":"HS256"}"#);
        let payload = B64_URL_SAFE.encode(r#"{"sub":"user123"}"#);
        let signature = B64_URL_SAFE.encode("dummy_signature");

        format!("{header}.{payload}.{signature}")
    }

    #[test]
    fn test_parse_jwt_expiry_valid_token() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let future_exp = now + 3600; // 1 hour in the future

        let token = create_test_jwt_with_exp(future_exp);
        let exp = parse_jwt_expiry(&token);

        assert_eq!(exp, Some(future_exp));
    }

    #[test]
    fn test_parse_jwt_expiry_no_exp_claim() {
        let token = create_test_jwt_without_exp();
        let exp = parse_jwt_expiry(&token);

        assert_eq!(exp, None);
    }

    #[test]
    fn test_parse_jwt_expiry_invalid_jwt_format() {
        // Not a JWT (missing dots)
        let token = "not.a.jwt.with.four.parts";
        let exp = parse_jwt_expiry(token);
        assert_eq!(exp, None);

        // Not a JWT (single dot)
        let exp = parse_jwt_expiry("not.jwt");
        assert_eq!(exp, None);

        // Not a JWT (no dots)
        let exp = parse_jwt_expiry("notajwt");
        assert_eq!(exp, None);
    }

    #[test]
    fn test_parse_jwt_expiry_invalid_base64() {
        // Invalid base64 in payload
        let token = "validheader.!!!invalid_base64!!!.validsignature";
        let exp = parse_jwt_expiry(token);
        assert_eq!(exp, None);
    }

    #[test]
    fn test_parse_jwt_expiry_invalid_json() {
        use base64::Engine;

        let header = B64_URL_SAFE.encode(r#"{"typ":"JWT","alg":"HS256"}"#);
        let payload = B64_URL_SAFE.encode("not valid json");
        let signature = B64_URL_SAFE.encode("dummy");

        let token = format!("{header}.{payload}.{signature}");
        let exp = parse_jwt_expiry(&token);

        assert_eq!(exp, None);
    }

    #[test]
    fn test_is_token_expired_past_token() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let past_exp = now - 3600; // 1 hour ago

        let expired = is_token_expired(past_exp);
        assert!(expired);
    }

    #[test]
    fn test_is_token_expired_future_token() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let future_exp = now + 3600; // 1 hour in the future

        let expired = is_token_expired(future_exp);
        assert!(!expired);
    }

    #[test]
    fn test_is_token_expired_within_clock_skew() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Expire 30 seconds ago (within the 60 second clock skew tolerance)
        let recent_exp = now - 30;
        let expired = is_token_expired(recent_exp);
        assert!(!expired);
    }

    #[test]
    fn test_is_token_expired_outside_clock_skew() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Expire 90 seconds ago (outside the 60 second clock skew tolerance)
        let old_exp = now - 90;
        let expired = is_token_expired(old_exp);
        assert!(expired);
    }

    #[test]
    fn test_is_token_expired_at_clock_skew_boundary() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Expire exactly at clock skew boundary (60 seconds ago)
        let boundary_exp = now - CLOCK_SKEW_TOLERANCE;
        let expired = is_token_expired(boundary_exp);

        // At the boundary, the token should be considered valid (now > exp + skew is false)
        assert!(!expired);
    }

    #[test]
    fn test_non_jwt_token_parse_gracefully() {
        // Test that non-JWT tokens (like API tokens) return None and don't panic
        let api_token = "flk_abcd1234efgh5678ijkl9012mnop";
        let exp = parse_jwt_expiry(api_token);

        // Should gracefully return None instead of panicking
        assert_eq!(exp, None);
    }
}
