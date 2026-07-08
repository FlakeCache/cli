# FlakeCache CLI

High-performance Nix binary cache client with FastCDC chunking.

## Installation

### Quick Install (Linux/macOS)

```bash
curl -fsSL https://git.infra.centralcloud.com/flakecache/cli/raw/branch/main/install.sh | sh
```

### Manual Download

Download the binary for your platform from `https://cache.flakecache.com/cli`.

| Platform | Binary |
|----------|--------|
| Linux x86_64 | `flakecache-linux-x86_64` |
| Linux ARM64 | `flakecache-linux-aarch64` |
| macOS Intel | `flakecache-macos-x86_64` |
| macOS Apple Silicon | `flakecache-macos-aarch64` |
| Windows x86_64 | `flakecache-windows-x86_64.exe` |
| Windows ARM64 | `flakecache-windows-aarch64.exe` |

### Forgejo/GitHub Actions

```yaml
- uses: FlakeCache/nix-installer@v1
  with:
    install-cli: 'true'
```

## Usage

```bash
# Login (OAuth)
flakecache login

# Push build outputs to cache
flakecache push ./result

# Pull from cache
flakecache pull /nix/store/xxx...

# Warm cache with flake outputs
flakecache warm .#packages.x86_64-linux.default
```

## Features

- 🚀 **FastCDC chunking** - Deduplicated storage across packages
- 🔐 **Ed25519 signing** - Cryptographic verification
- 📦 **Zstd compression** - Efficient storage
- ⚡ **Parallel uploads** - Fast multi-threaded transfers
- 🔄 **Daemon mode** - Background sync

## Source Code

Source code is maintained in
[flakecache/cli](https://git.infra.centralcloud.com/flakecache/cli), with a
GitHub mirror at [FlakeCache/cli](https://github.com/FlakeCache/cli).

## License

Apache-2.0
