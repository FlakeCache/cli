//! Authentication configuration

use serde::{Deserialize, Serialize};

/// Authentication configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Access token for FlakeCache API
    #[serde(default)]
    pub token: String,

    /// Refresh token for token renewal
    #[serde(default)]
    pub refresh_token: String,

    /// User's username/email
    #[serde(default)]
    pub username: String,

    /// Token expiration timestamp (Unix seconds)
    #[serde(default)]
    pub expires_at: Option<u64>,
}

impl AuthConfig {
    /// Check if authentication is configured
    pub fn is_authenticated(&self) -> bool {
        !self.token.is_empty()
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|dur| dur.as_secs() >= expires_at)
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// Check if token needs refresh
    pub fn needs_refresh(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            // Refresh if within 5 minutes of expiration
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|dur| dur.as_secs() + 300 >= expires_at)
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// Clear authentication
    pub fn clear(&mut self) {
        self.token.clear();
        self.refresh_token.clear();
        self.username.clear();
        self.expires_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authenticated() {
        let auth = AuthConfig {
            token: "abc123".to_string(),
            ..Default::default()
        };
        assert!(auth.is_authenticated());
    }

    #[test]
    fn test_not_authenticated() {
        let auth = AuthConfig::default();
        assert!(!auth.is_authenticated());
    }
}
