#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # FlakeCache CLI
//!
//! A production-grade command-line tool for managing Nix binary caches with FlakeCache.
//!
//! ## Architecture
//!
//! This library is organized into several key modules:
//!
//! - **[`error`]** - Error types and error handling
//! - **[`config`]** - Configuration management and credential storage
//! - **[`client`]** - HTTP client abstraction and CBOR protocol
//! - **[`commands`]** - Command implementations (push, pull, auth, etc.)
//! - **[`cache`]** - Cache operations (signing, transfer, warming)
//! - **[`nix`]** - Nix store and flake integration
//! - **[`utils`]** - Utilities (progress tracking, parallelization, etc.)
//!
//! ## Quick Start
//!
//! ```bash
//! flakecache login              # Authenticate
//! flakecache push --cache myapp # Upload build artifacts
//! flakecache pull               # Download and populate cache
//! ```

pub mod cache;
pub mod client;
pub mod commands;
pub mod config;
pub mod error;
pub mod nix;
pub mod utils;

/// Error type alias for convenience
pub use error::{CliError, Result};

/// Configuration type alias for convenience
pub use config::Config;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Library name
pub const NAME: &str = "flakecache";
