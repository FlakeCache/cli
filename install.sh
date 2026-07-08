#!/bin/sh
set -e

# FlakeCache CLI Installer
# Usage: curl -fsSL https://git.infra.centralcloud.com/flakecache/cli/raw/branch/main/install.sh | sh

INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BASE_URL="${FLAKECACHE_CLI_BASE_URL:-https://cache.flakecache.com/cli}"

echo "🚀 Installing FlakeCache CLI..."

# Detect OS
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
linux) PLATFORM="linux" ;;
darwin) PLATFORM="macos" ;;
mingw* | msys* | cygwin*) PLATFORM="windows" ;;
*)
	echo "❌ Unsupported OS: $OS"
	exit 1
	;;
esac

case "$ARCH" in
x86_64 | amd64) ARCH="x86_64" ;;
aarch64 | arm64) ARCH="aarch64" ;;
*)
	echo "❌ Unsupported architecture: $ARCH"
	exit 1
	;;
esac

BINARY="flakecache-${PLATFORM}-${ARCH}"
if [ "$PLATFORM" = "windows" ]; then
	BINARY="${BINARY}.exe"
fi

echo "📥 Downloading FlakeCache CLI ($BINARY)..."

DOWNLOAD_URL="${BASE_URL}/${BINARY}"
CHECKSUMS_URL="${BASE_URL}/checksums.sha256"
TMP_FILE=$(mktemp)
CHECKSUMS_FILE=$(mktemp)
trap 'rm -f "$TMP_FILE" "$CHECKSUMS_FILE"' EXIT

curl -fsSL "$DOWNLOAD_URL" -o "$TMP_FILE" || {
	echo "❌ Download failed"
	exit 1
}

curl -fsSL "$CHECKSUMS_URL" -o "$CHECKSUMS_FILE" || {
	echo "❌ Checksum download failed"
	exit 1
}

if command -v sha256sum >/dev/null 2>&1; then
	expected=$(grep -E "(^|[[:space:]])${BINARY}$" "$CHECKSUMS_FILE" | awk '{print $1}')
	if [ -z "$expected" ]; then
		echo "❌ Checksum not found for $BINARY"
		exit 1
	fi
	printf '%s  %s\n' "$expected" "$(basename "$TMP_FILE")" | (cd "$(dirname "$TMP_FILE")" && sha256sum --check --strict) || {
		echo "❌ Checksum verification failed"
		exit 1
	}
elif command -v shasum >/dev/null 2>&1; then
	expected=$(grep -E "(^|[[:space:]])${BINARY}$" "$CHECKSUMS_FILE" | awk '{print $1}')
	if [ -z "$expected" ]; then
		echo "❌ Checksum not found for $BINARY"
		exit 1
	fi
	actual=$(shasum -a 256 "$TMP_FILE" | awk '{print $1}')
	if [ "$expected" != "$actual" ]; then
		echo "❌ Checksum verification failed"
		exit 1
	fi
else
	echo "❌ sha256sum or shasum is required for checksum verification"
	exit 1
fi

chmod +x "$TMP_FILE"

# Install
if [ -w "$INSTALL_DIR" ]; then
	mv "$TMP_FILE" "$INSTALL_DIR/flakecache"
else
	echo "📦 Installing to $INSTALL_DIR (requires sudo)..."
	sudo mv "$TMP_FILE" "$INSTALL_DIR/flakecache"
fi

echo "✅ FlakeCache CLI installed to $INSTALL_DIR/flakecache"
echo ""
flakecache --version 2>/dev/null || true
echo ""
echo "Get started:"
echo "  flakecache login     # Authenticate"
echo "  flakecache push      # Push to cache"
echo "  flakecache --help    # Show all commands"
