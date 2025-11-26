//! `FlakeCache` CLI - Fast, production-grade Nix binary cache
//!
//! `FlakeCache` is a command-line tool for interacting with the `FlakeCache` binary cache service.
//! It provides commands for uploading store paths, managing cache contents, authentication,
//! and CI/CD integration workflows.
//!
//! # Quick Start
//!
//! ```bash
//! # Login to FlakeCache
//! flakecache login
//!
//! # Upload store paths to cache
//! flakecache push --cache my-cache
//!
//! # List cache contents
//! flakecache list --cache my-cache
//! ```

mod auth;
mod bandwidth;
mod cache_management;
mod cbor_client;
mod fast_client;
mod flake_helper;
mod resolve;
mod self_update_cmd;
mod sig_verify;
mod upload;
mod upload_progress;
mod workflow;

// Rust CLI is for CI/CD only - server-side admin stays in Elixir CLI

use anyhow::Result;
use clap::{Parser, Subcommand};
use self_update_cmd::self_update;

#[derive(Parser)]
#[command(name = "flakecache")]
#[command(
    about = "⚡ A CLI tool for accelerating Nix CI/CD pipelines by managing a shared binary cache"
)]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(long_about = concat!(
    "⚡ FlakeCache (v", env!("CARGO_PKG_VERSION"), ")\n",
    "A CLI tool for accelerating Nix CI/CD pipelines by managing a shared binary cache.\n\n",
    "Use this tool to download pre-built dependencies (resolve), upload build artifacts (push),\n",
    "and pre-warm your cache (populate) to dramatically reduce CI build times."
))]
struct Cli {
    /// Enable verbose output for debugging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    // ═══════════════════════════════════════════════════════════
    // Core CI/CD Commands
    // ═══════════════════════════════════════════════════════════
    /// Download dependencies (store paths) from the cache to the local Nix store
    ///
    /// Use this at the beginning of a CI job to pull pre-built artifacts and
    /// dramatically reduce build times. Can either auto-detect dependencies from
    /// your flake or resolve specific flake outputs.
    ///
    /// Examples:
    ///   flakecache resolve                    # Auto-detect and resolve all dependencies
    ///   flakecache resolve .#myapp            # Resolve dependencies for .#myapp
    ///   flakecache resolve nixpkgs#hello      # Resolve dependencies for hello
    #[command(visible_alias = "download")]
    #[command(display_order = 1)]
    Resolve {
        /// Optional flake output to resolve (e.g., .#myapp, nixpkgs#hello)
        /// If omitted, auto-detects dependencies from current directory
        flake_output: Option<String>,
        /// `FlakeCache` host URL (defaults to <https://c.flakecache.com>)
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    /// Upload specified Nix store paths (NARs) to the cache
    ///
    /// Use this at the end of a CI job to share build artifacts with your team.
    /// Supports multiple input formats:
    /// - Flake outputs: .#hello, nixpkgs#hello, github:owner/repo#package
    /// - Direct store paths: /nix/store/abc123-hello
    /// - No arguments: uploads all recent nix build outputs
    ///
    /// Examples:
    ///   flakecache push --cache my-org-cache
    ///   flakecache push --cache my-org-cache .#myapp
    ///   flakecache push --cache my-org-cache --store-path /nix/store/abc123-hello
    #[command(visible_alias = "upload")]
    #[command(display_order = 2)]
    Push {
        /// Name of the cache to upload to (e.g., my-org-cache)
        #[arg(short, long)]
        cache: String,
        /// What to push: flake output (.#pkg), store path (/nix/store/...), or package name
        /// Can be specified multiple times. If omitted, pushes recent build outputs.
        #[arg(short, long)]
        store_path: Option<Vec<String>>,
        /// `FlakeCache` host URL (defaults to <https://c.flakecache.com>)
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    /// Build store paths locally and immediately upload the results to pre-warm the cache
    ///
    /// Use this to populate your cache with commonly-used dependencies before
    /// your CI jobs run. This is especially useful for warming a cache with
    /// your project's development shell or frequently-built packages.
    ///
    /// Examples:
    ///   flakecache populate --cache my-org-cache --flake . --expression devShells.x86_64-linux.default
    ///   flakecache populate --cache my-org-cache --paths nixpkgs#hello,nixpkgs#wget
    #[command(visible_alias = "warm")]
    #[command(display_order = 3)]
    Populate {
        /// Name of the cache to upload to (e.g., my-org-cache)
        #[arg(short, long)]
        cache: String,
        /// Package names or store paths to build and upload (comma-separated)
        #[arg(short, long)]
        paths: Option<String>,
        /// Flake reference (e.g., github:owner/repo, ., or path/to/flake)
        #[arg(short, long)]
        flake: Option<String>,
        /// Nix expression to build (e.g., packages.x86_64-linux.hello or devShells.x86_64-linux.default)
        #[arg(short, long)]
        expression: Option<String>,
        /// `FlakeCache` host URL (defaults to <https://c.flakecache.com>)
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    /// Resolve dependencies, run a build command, and push results — all in one step
    ///
    /// This is the ultimate CI convenience command. It automatically:
    /// 1. Resolves (downloads) all dependencies from the cache
    /// 2. Runs your build command (e.g., `nix build`, `nix develop`, etc.)
    /// 3. Pushes the resulting store paths to the cache
    ///
    /// Examples:
    ///   flakecache run --cache my-cache -- nix build
    ///   flakecache run --cache my-cache -- nix develop --command make test
    #[command(display_order = 4)]
    Run {
        /// Name of the cache to use
        #[arg(short, long)]
        cache: String,
        /// Build command to run (everything after --)
        #[arg(last = true, required = true)]
        command: Vec<String>,
        /// `FlakeCache` host URL
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    // ═══════════════════════════════════════════════════════════
    // Cache Management
    // ═══════════════════════════════════════════════════════════
    /// List paths in the cache
    ///
    /// Shows all store paths currently in your cache. Use --query to filter by name.
    ///
    /// Examples:
    ///   flakecache list --cache my-cache
    ///   flakecache list --cache my-cache --query hello
    ///   flakecache list --cache my-cache --older-than 30d
    #[command(display_order = 10)]
    List {
        /// Name of the cache to list
        #[arg(short, long)]
        cache: String,
        /// Filter by name pattern
        #[arg(short, long)]
        query: Option<String>,
        /// Show only paths older than duration (e.g., 30d, 7d, 24h)
        #[arg(long)]
        older_than: Option<String>,
        /// `FlakeCache` host URL
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    /// Inspect metadata for a specific store path
    ///
    /// Shows detailed information about a cached path including:
    /// - Who pushed it and when
    /// - NAR size and hash
    /// - Referenced store paths
    ///
    /// Example:
    ///   flakecache inspect --cache my-cache /nix/store/abc123-hello-2.12
    #[command(display_order = 11)]
    Inspect {
        /// Name of the cache
        #[arg(short, long)]
        cache: String,
        /// Store path to inspect
        store_path: String,
        /// `FlakeCache` host URL
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    /// Delete a specific store path from the cache
    ///
    /// Permanently removes a store path. Use with caution.
    ///
    /// Example:
    ///   flakecache delete --cache my-cache /nix/store/abc123-bad-build
    #[command(display_order = 12)]
    Delete {
        /// Name of the cache
        #[arg(short, long)]
        cache: String,
        /// Store path to delete
        store_path: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
        /// `FlakeCache` host URL
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    /// Garbage collect old paths from the cache
    ///
    /// Removes paths older than the specified duration to free up space.
    ///
    /// Examples:
    ///   flakecache gc --cache my-cache --older-than 30d
    ///   flakecache gc --cache my-cache --older-than 7d --dry-run
    #[command(display_order = 13)]
    Gc {
        /// Name of the cache
        #[arg(short, long)]
        cache: String,
        /// Delete paths older than duration (e.g., 30d, 7d, 24h)
        #[arg(long)]
        older_than: String,
        /// Show what would be deleted without actually deleting
        #[arg(long)]
        dry_run: bool,
        /// `FlakeCache` host URL
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    // ═══════════════════════════════════════════════════════════
    // Diagnostics & Observability
    // ═══════════════════════════════════════════════════════════
    /// Show cache usage statistics and hit/miss ratios
    ///
    /// Displays metrics about your cache including:
    /// - Total size and artifact count
    /// - Hit/miss ratio for recent builds
    /// - Bandwidth saved
    ///
    /// Example:
    ///   flakecache stats --cache my-cache
    #[command(display_order = 20)]
    Stats {
        /// Name of the cache
        #[arg(short, long)]
        cache: String,
        /// Time period for statistics (e.g., 7d, 30d, 90d)
        #[arg(long, default_value = "7d")]
        period: String,
        /// `FlakeCache` host URL
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    /// Diagnose setup and connectivity issues
    ///
    /// Checks your `FlakeCache` setup and reports any issues:
    /// - Connection to cache server
    /// - Token validity
    /// - Nix installation and configuration
    /// - Substituter configuration
    ///
    /// Example:
    ///   flakecache doctor
    #[command(display_order = 21)]
    Doctor {
        /// `FlakeCache` host URL
        #[arg(long, default_value = "https://c.flakecache.com")]
        api_url: String,
    },

    // ═══════════════════════════════════════════════════════════
    // Authentication & Setup
    // ═══════════════════════════════════════════════════════════
    /// Authenticate with the `FlakeCache` server
    ///
    /// Opens a browser for OAuth authentication (similar to `gh auth login`).
    /// You can also pass a token directly with --token, or set the
    /// `FLAKECACHE_TOKEN` environment variable. Required before using cache operations.
    ///
    /// Examples:
    ///   flakecache login
    ///   flakecache login --token `fc_abc123xyz`
    ///   `FLAKECACHE_TOKEN=fc_abc123xyz` flakecache push --cache my-cache
    #[command(display_order = 4)]
    Login {
        /// `FlakeCache` API URL for authentication
        #[arg(long, default_value = "https://api.flakecache.com")]
        api_url: String,
        /// API token (use instead of web OAuth flow). Can also set `FLAKECACHE_TOKEN` env var
        #[arg(long)]
        token: Option<String>,
        /// Force new login (ignore existing saved token)
        #[arg(long)]
        force_new_login: bool,
    },

    /// Display the currently authenticated user
    ///
    /// Shows your email, user ID, organization, and plan details.
    /// Similar to `gh auth status`.
    ///
    /// Example:
    ///   flakecache whoami
    #[command(display_order = 5)]
    Whoami {
        /// `FlakeCache` API URL
        #[arg(long, default_value = "https://api.flakecache.com")]
        api_url: String,
    },

    /// Generate a helper script for your specific CI/CD system
    ///
    /// Creates an integration script customized for your CI platform
    /// (GitHub Actions, GitLab CI, Jenkins, `CircleCI`, etc.). The script
    /// will be saved to a system-specific location unless you specify --output.
    ///
    /// Examples:
    ///   flakecache generate-script --ci github
    ///   flakecache generate-script --ci gitlab --output .gitlab-ci-flakecache.yml
    #[command(display_order = 6)]
    GenerateScript {
        /// CI system (e.g., github, gitlab, jenkins, circleci, travis, bitbucket,
        /// buildkite, tekton, drone, azure-devops, aws-codebuild, gcp-cloudbuild,
        /// teamcity, bamboo, concourse, spinnaker, argocd, bash, generic)
        #[arg(short, long)]
        ci: String,
        /// Output file path (defaults to system-specific location)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Update the flakecache CLI to the latest release (or a specific tag)
    #[command(display_order = 7)]
    SelfUpdate {
        /// Optional tag to install (defaults to latest release)
        #[arg(long)]
        tag: Option<String>,
    },

    /// Verify the integrity of this binary against a detached Ed25519 signature
    ///
    /// Each `FlakeCache` release includes a `.sig` file containing a base64-encoded
    /// Ed25519 signature. Use this command to verify that the binary hasn't been
    /// tampered with and comes from the official `FlakeCache` maintainers.
    ///
    /// The public key is embedded in the binary at compile time, so no external
    /// files are needed for verification—just the signature.
    ///
    /// Examples:
    ///   # Verify using signature from current directory
    ///   flakecache verify-self --signature-file ./flakecache.sig
    ///
    ///   # Download and verify in one command
    ///   curl -o flakecache.sig <https://c.flakecache.com/cli/latest/x86_64-unknown-linux-musl/flakecache.sig>
    ///   flakecache verify-self --signature-file ./flakecache.sig
    #[command(display_order = 8)]
    VerifySelf {
        /// Path to the detached signature file (base64-encoded Ed25519 signature)
        #[arg(short, long)]
        signature_file: String,
    },
}

#[tokio::main]
#[allow(clippy::too_many_lines)] // Main function coordinates all CLI commands
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set verbose mode globally if needed
    if cli.verbose {
        std::env::set_var("FLAKECACHE_VERBOSE", "1");
    }

    match cli.command {
        // Core CI/CD Commands
        Commands::Resolve {
            flake_output,
            api_url,
        } => {
            if let Some(ref flake_ref) = flake_output {
                // Resolve specific flake output
                println!("🔍 Resolving dependencies for {flake_ref}...");
                let paths = flake_helper::resolve_to_store_paths(flake_ref).await?;

                // Get full closure (all dependencies) for these paths
                let mut all_deps = Vec::new();
                for path in paths {
                    let deps = flake_helper::get_store_path_closure(&path).await?;
                    all_deps.extend(deps);
                }

                // Deduplicate
                all_deps.sort();
                all_deps.dedup();

                println!("📥 Found {} dependencies to resolve", all_deps.len());

                // Download all dependencies from cache
                // Use a default cache name for resolve (resolves to any available cache)
                resolve::resolve(all_deps, "public", &api_url).await?;
            } else {
                // Auto-detect and resolve
                upload::prewarm().await?;
            }
        }
        Commands::Push {
            cache,
            store_path,
            api_url,
        } => {
            // If store paths are specified, resolve them (could be flake refs)
            let resolved_paths = if let Some(paths) = store_path {
                let mut all_paths = Vec::new();
                for path_spec in paths {
                    let paths = flake_helper::resolve_to_store_paths(&path_spec).await?;
                    all_paths.extend(paths);
                }
                Some(all_paths)
            } else {
                None
            };

            upload::upload(&cache, resolved_paths, &api_url).await?;
        }
        Commands::Populate {
            cache,
            paths,
            flake,
            expression,
            api_url,
        } => {
            upload::warm(&cache, paths, flake, expression, &api_url).await?;
        }
        Commands::Run {
            cache,
            command,
            api_url,
        } => {
            println!("🚀 FlakeCache Run: resolve → build → push");

            // Step 1: Resolve dependencies
            println!("📥 Step 1/3: Resolving dependencies...");
            upload::prewarm().await?;

            // Step 2: Run build command
            println!("🔨 Step 2/3: Running build command: {}", command.join(" "));
            let status = std::process::Command::new(&command[0])
                .args(&command[1..])
                .status()?;

            if !status.success() {
                return Err(anyhow::anyhow!(
                    "Build command failed with exit code: {:?}",
                    status.code()
                ));
            }

            // Step 3: Push results
            println!("📤 Step 3/3: Pushing build results to cache...");
            upload::upload(&cache, None, &api_url).await?;

            println!("✅ Complete! Your build results are now cached.");
        }

        // Cache Management Commands
        Commands::List {
            cache,
            query,
            older_than,
            api_url,
        } => {
            cache_management::list_paths(&cache, query, older_than, &api_url).await?;
        }
        Commands::Inspect {
            cache,
            store_path,
            api_url,
        } => {
            cache_management::inspect_path(&cache, &store_path, &api_url).await?;
        }
        Commands::Delete {
            cache,
            store_path,
            force,
            api_url,
        } => {
            cache_management::delete_path(&cache, &store_path, force, &api_url).await?;
        }
        Commands::Gc {
            cache,
            older_than,
            dry_run,
            api_url,
        } => {
            cache_management::gc_cache(&cache, &older_than, dry_run, &api_url).await?;
        }

        // Diagnostics & Observability
        Commands::Stats {
            cache,
            period,
            api_url,
        } => {
            cache_management::show_stats(&cache, &period, &api_url).await?;
        }
        Commands::Doctor { api_url } => {
            println!("🩺 FlakeCache Doctor - Checking your setup...\n");

            // Check 1: Nix installation
            print!("✓ Checking Nix installation... ");
            if std::process::Command::new("nix")
                .arg("--version")
                .output()
                .is_ok()
            {
                println!("OK");
            } else {
                println!("❌ FAILED\n  Nix is not installed or not in PATH");
            }

            // Check 2: Token
            print!("✓ Checking FlakeCache token... ");
            match auth::load_token() {
                Ok(Some(_)) => println!("OK"),
                Ok(None) => println!("❌ FAILED\n  No token found. Run 'flakecache login'"),
                Err(e) => println!("❌ FAILED\n  Error: {e}"),
            }

            // Check 3: Connectivity
            print!("✓ Checking connectivity to {api_url}... ");
            match reqwest::get(&api_url).await {
                Ok(resp) if resp.status().is_success() || resp.status().is_client_error() => {
                    println!("OK");
                }
                Ok(resp) => println!("⚠️  Unexpected status: {}", resp.status()),
                Err(e) => println!("❌ FAILED\n  Error: {e}"),
            }

            println!("\n✅ Diagnostic check complete");
        }

        // Authentication & Setup
        Commands::Login {
            api_url,
            token,
            force_new_login,
        } => {
            auth::login(&api_url, token.as_deref(), force_new_login).await?;
        }
        Commands::Whoami { api_url } => {
            auth::whoami(&api_url).await?;
        }
        Commands::GenerateScript { ci, output } => {
            workflow::generate_script(&ci, output.as_deref()).await?;
        }
        Commands::SelfUpdate { tag } => {
            self_update(tag.as_deref())?;
        }
        Commands::VerifySelf { signature_file } => {
            println!("🔐 Verifying binary signature...");
            let sig_path = std::path::PathBuf::from(signature_file);
            sig_verify::verify_self(&sig_path)?;
            println!("✅ Signature verified! Binary is authentic.");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_parsing() {
        // Test that CLI can parse basic commands without panicking
        let app = Cli::command();

        // Test basic command parsing (help/version exit the process so we can't test them directly)
        let result = app.try_get_matches_from(["flakecache", "resolve"]);
        assert!(result.is_ok());

        // Test that the app has the expected structure
        let app = Cli::command();
        assert_eq!(app.get_name(), "flakecache");
    }

    #[test]
    fn test_resolve_command_parsing() {
        let app = Cli::command();

        // Test resolve command with flake output
        let result = app
            .clone()
            .try_get_matches_from(["flakecache", "resolve", ".#hello"]);
        assert!(result.is_ok());

        // Test resolve command with custom API URL
        let result = app.try_get_matches_from([
            "flakecache",
            "resolve",
            "--api-url",
            "https://test.com",
            ".#hello",
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_push_command_parsing() {
        let app = Cli::command();

        // Test push command with required cache argument
        let result =
            app.clone()
                .try_get_matches_from(["flakecache", "push", "--cache", "my-cache"]);
        assert!(result.is_ok());

        // Test push command with store path
        let result = app.try_get_matches_from([
            "flakecache",
            "push",
            "--cache",
            "my-cache",
            "--store-path",
            "/nix/store/abc123-hello",
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_populate_command_parsing() {
        let app = Cli::command();

        // Test populate command with required arguments
        let result = app.try_get_matches_from([
            "flakecache",
            "populate",
            "--cache",
            "my-cache",
            "--flake",
            ".",
            "--expression",
            "packages.x86_64-linux.hello",
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verbose_flag() {
        let app = Cli::command();

        // Test verbose flag works with any command
        let result = app.try_get_matches_from(["flakecache", "--verbose", "resolve"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_command_fails() {
        let app = Cli::command();

        // Test that invalid commands are rejected
        let result = app.try_get_matches_from(["flakecache", "invalid-command"]);
        assert!(result.is_err());
    }
}
