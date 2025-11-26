//! Error types and handling for FlakeCache CLI
//!
//! Provides structured error types for all CLI operations with proper context
//! and error chains for debugging.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for FlakeCache CLI operations
pub type Result<T> = std::result::Result<T, CliError>;

/// Comprehensive error types for FlakeCache CLI operations
#[derive(Error, Debug)]
pub enum CliError {
    // ═══════════════════════════════════════════════════════════════
    // Network & HTTP Errors
    // ═══════════════════════════════════════════════════════════════
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    Http(String),

    /// Failed to connect to FlakeCache server
    #[error("Failed to connect to {host}: {reason}")]
    ConnectionError { host: String, reason: String },

    /// API error response from server
    #[error("FlakeCache API error: {status} - {message}")]
    ApiError { status: u16, message: String },

    /// Invalid API response format
    #[error("Invalid API response: {0}")]
    InvalidResponse(String),

    // ═══════════════════════════════════════════════════════════════
    // Authentication & Authorization
    // ═══════════════════════════════════════════════════════════════
    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    /// Authentication token is missing or invalid
    #[error("Missing or invalid authentication token. Run 'flakecache login' first")]
    MissingToken,

    /// OAuth flow error
    #[error("OAuth flow error: {0}")]
    OAuthError(String),

    /// Token expired or invalid
    #[error("Token expired or invalid: {0}")]
    TokenExpired(String),

    // ═══════════════════════════════════════════════════════════════
    // Configuration & File Errors
    // ═══════════════════════════════════════════════════════════════
    /// Failed to read configuration file
    #[error("Failed to read config from {path}: {reason}")]
    ConfigRead { path: PathBuf, reason: String },

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Configuration file not found
    #[error("Configuration not found. Run 'flakecache login' to set up credentials")]
    NoConfig,

    /// Failed to write configuration file
    #[error("Failed to write config to {path}: {reason}")]
    ConfigWrite { path: PathBuf, reason: String },

    // ═══════════════════════════════════════════════════════════════
    // Nix & Store Errors
    // ═══════════════════════════════════════════════════════════════
    /// Failed to interact with Nix store
    #[error("Nix store error: {0}")]
    StoreError(String),

    /// Failed to resolve flake
    #[error("Failed to resolve flake {flake}: {reason}")]
    FlakeResolutionError { flake: String, reason: String },

    /// Invalid Nix store path
    #[error("Invalid store path: {path}")]
    InvalidStorePath { path: String },

    /// Flake not found
    #[error("Flake not found: {flake}")]
    FlakeNotFound { flake: String },

    /// Store path not found in local store
    #[error("Store path not found in local Nix store: {path}")]
    StorePathNotFound { path: String },

    // ═══════════════════════════════════════════════════════════════
    // Cache Operations
    // ═══════════════════════════════════════════════════════════════
    /// Cache operation failed
    #[error("Cache operation failed: {0}")]
    CacheError(String),

    /// NAR (Nix ARchive) signing/verification failed
    #[error("NAR signature verification failed: {0}")]
    SignatureError(String),

    /// Cache not found or access denied
    #[error("Cache '{cache}' not found or access denied")]
    CacheNotFound { cache: String },

    /// Invalid cache name
    #[error("Invalid cache name: {name}")]
    InvalidCacheName { name: String },

    // ═══════════════════════════════════════════════════════════════
    // Upload/Download Errors
    // ═══════════════════════════════════════════════════════════════
    /// Upload failed
    #[error("Upload failed: {0}")]
    UploadFailed(String),

    /// Download failed
    #[error("Download failed: {0}")]
    DownloadFailed(String),

    /// Transfer interrupted
    #[error("Transfer interrupted: {0}")]
    TransferInterrupted(String),

    /// Checksum mismatch
    #[error("Checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    // ═══════════════════════════════════════════════════════════════
    // Serialization & Encoding Errors
    // ═══════════════════════════════════════════════════════════════
    /// Failed to serialize data
    #[error("Serialization failed: {0}")]
    SerializationError(String),

    /// Failed to deserialize data
    #[error("Deserialization failed: {0}")]
    DeserializationError(String),

    /// Invalid encoding
    #[error("Invalid encoding: {0}")]
    EncodingError(String),

    // ═══════════════════════════════════════════════════════════════
    // I/O Errors
    // ═══════════════════════════════════════════════════════════════
    /// File operation failed
    #[error("File operation failed: {path}: {reason}")]
    FileError { path: PathBuf, reason: String },

    /// Directory operation failed
    #[error("Directory operation failed: {path}: {reason}")]
    DirError { path: PathBuf, reason: String },

    /// Permission denied
    #[error("Permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    // ═══════════════════════════════════════════════════════════════
    // Validation & Input Errors
    // ═══════════════════════════════════════════════════════════════
    /// Invalid input argument
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Missing required argument
    #[error("Missing required argument: {0}")]
    MissingArgument(String),

    // ═══════════════════════════════════════════════════════════════
    // Other Errors
    // ═══════════════════════════════════════════════════════════════
    /// Generic internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Timeout
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Cancelled by user
    #[error("Operation cancelled")]
    Cancelled,
}

impl CliError {
    /// Get the exit code for this error
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::MissingToken | Self::NoConfig => 1,
            Self::InvalidArgument(_) | Self::MissingArgument(_) => 2,
            Self::AuthFailed(_) | Self::OAuthError(_) => 3,
            Self::ConnectionError { .. } | Self::Http(_) => 4,
            Self::StoreError(_) | Self::FlakeResolutionError { .. } => 5,
            Self::CacheError(_) | Self::CacheNotFound { .. } => 6,
            Self::UploadFailed(_) | Self::DownloadFailed(_) => 7,
            Self::PermissionDenied { .. } => 13,
            Self::Timeout(_) => 124,
            Self::Cancelled => 130,
            _ => 1,
        }
    }

    /// Whether the error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionError { .. }
                | Self::Http(_)
                | Self::DownloadFailed(_)
                | Self::UploadFailed(_)
                | Self::TransferInterrupted(_)
                | Self::Timeout(_)
        )
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::FileError {
                path: PathBuf::from("<unknown>"),
                reason: "Not found".to_string(),
            },
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied {
                path: PathBuf::from("<unknown>"),
            },
            _ => Self::FileError {
                path: PathBuf::from("<unknown>"),
                reason: err.to_string(),
            },
        }
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        if err.is_io() {
            Self::FileError {
                path: PathBuf::from("<unknown>"),
                reason: err.to_string(),
            }
        } else if err.is_syntax() {
            Self::DeserializationError(format!("JSON syntax error: {err}"))
        } else {
            Self::DeserializationError(err.to_string())
        }
    }
}

impl From<ciborium::de::Error<std::io::Error>> for CliError {
    fn from(err: ciborium::de::Error<std::io::Error>) -> Self {
        Self::DeserializationError(format!("CBOR decode error: {err}"))
    }
}

impl From<ciborium::ser::Error<std::io::Error>> for CliError {
    fn from(err: ciborium::ser::Error<std::io::Error>) -> Self {
        Self::SerializationError(format!("CBOR encode error: {err}"))
    }
}
