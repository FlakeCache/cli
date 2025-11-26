//! Command-line interface argument parsing
//!
//! Defines all CLI commands and their arguments using Clap.

use clap::{Parser, Subcommand};

/// FlakeCache CLI - Fast, production-grade Nix binary cache client
#[derive(Parser, Debug)]
#[command(name = "flakecache")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "⚡ A CLI tool for accelerating Nix CI/CD pipelines by managing a shared binary cache")]
#[command(long_about = concat!(
    "⚡ FlakeCache (v", env!("CARGO_PKG_VERSION"), ")\n",
    "A CLI tool for accelerating Nix CI/CD pipelines by managing a shared binary cache.\n\n",
    "Use this tool to download pre-built dependencies (pull), upload build artifacts (push),\n",
    "and manage authentication with 'login'."
))]
pub struct Cli {
    /// Enable verbose output for debugging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// FlakeCache server URL
    #[arg(long, global = true, default_value = "https://c.flakecache.com")]
    pub api_url: String,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Authenticate with FlakeCache
    ///
    /// Interactive login flow via OAuth. Saves credentials to ~/.config/flakecache/config.toml
    ///
    /// Example:
    ///   flakecache login
    #[command(visible_alias = "auth")]
    #[command(display_order = 1)]
    Login {
        /// Optional cache name to use by default
        #[arg(long)]
        cache: Option<String>,
    },

    /// Logout and clear saved credentials
    ///
    /// Example:
    ///   flakecache logout
    #[command(display_order = 2)]
    Logout,

    /// Download dependencies from the cache
    ///
    /// Pulls pre-built store paths from FlakeCache to the local Nix store.
    /// Can auto-detect dependencies or resolve specific flake outputs.
    ///
    /// Examples:
    ///   flakecache pull                    # Auto-detect and pull all dependencies
    ///   flakecache pull .#myapp            # Pull dependencies for .#myapp
    ///   flakecache pull nixpkgs#hello      # Pull dependencies for hello
    #[command(visible_alias = "download")]
    #[command(visible_alias = "resolve")]
    #[command(display_order = 3)]
    Pull {
        /// Optional flake output to resolve (e.g., .#myapp, nixpkgs#hello)
        /// If omitted, auto-detects dependencies from current directory
        flake_output: Option<String>,

        /// Name of the cache to pull from
        #[arg(long)]
        cache: Option<String>,

        /// Maximum parallel downloads
        #[arg(long)]
        parallelism: Option<usize>,
    },

    /// Upload build artifacts to the cache
    ///
    /// Uploads specified store paths (NARs) to FlakeCache.
    /// Supports multiple input formats.
    ///
    /// Examples:
    ///   flakecache push --cache my-cache
    ///   flakecache push --cache my-cache .#myapp
    ///   flakecache push --cache my-cache --store-path /nix/store/abc123-hello
    #[command(visible_alias = "upload")]
    #[command(display_order = 4)]
    Push {
        /// Name of the cache to push to (required)
        #[arg(long, required = true)]
        cache: String,

        /// Optional flake output to push (e.g., .#hello)
        /// If omitted, uploads all recent build outputs
        flake_output: Option<String>,

        /// Specific store path to upload
        #[arg(long)]
        store_path: Option<String>,

        /// Maximum parallel uploads
        #[arg(long)]
        parallelism: Option<usize>,

        /// Skip signature verification
        #[arg(long)]
        skip_verification: bool,
    },

    /// List contents of a cache
    ///
    /// Display all store paths currently in the cache.
    ///
    /// Examples:
    ///   flakecache list --cache my-cache
    ///   flakecache list --cache my-cache --limit 50
    #[command(display_order = 5)]
    List {
        /// Name of the cache to list
        #[arg(long, required = true)]
        cache: String,

        /// Maximum number of results
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Pagination cursor (from previous result)
        #[arg(long)]
        after: Option<String>,
    },

    /// Warm the cache with commonly-used store paths
    ///
    /// Pre-populate cache with dependencies to speed up future builds.
    ///
    /// Examples:
    ///   flakecache warm --cache my-cache
    #[command(display_order = 6)]
    Warm {
        /// Name of the cache to warm
        #[arg(long, required = true)]
        cache: String,

        /// Maximum parallel downloads
        #[arg(long)]
        parallelism: Option<usize>,
    },

    /// Show cache statistics and usage
    ///
    /// Examples:
    ///   flakecache stats --cache my-cache
    #[command(display_order = 7)]
    Stats {
        /// Name of the cache
        #[arg(long, required = true)]
        cache: String,
    },

    /// Check CLI version
    ///
    /// Examples:
    ///   flakecache version
    #[command(display_order = 8)]
    Version,
}

impl Cli {
    /// Parse command-line arguments
    ///
    /// # Returns
    ///
    /// Parsed CLI arguments
    pub fn parse_args() -> Self {
        <Self as Parser>::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
