#!/bin/sh
set -e

# FlakeCache CLI Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/FlakeCache/cli/main/install.sh | sh

REPO="FlakeCache/cli"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

echo "🚀 Installing FlakeCache CLI..."

# Detect OS
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  linux)  PLATFORM="linux" ;;
  darwin) PLATFORM="macos" ;;
  mingw*|msys*|cygwin*) PLATFORM="windows" ;;
  *)      echo "❌ Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)  ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *)             echo "❌ Unsupported architecture: $ARCH"; exit 1 ;;
esac

BINARY="flakecache-${PLATFORM}-${ARCH}"
if [ "$PLATFORM" = "windows" ]; then
  BINARY="${BINARY}.exe"
fi

# Get latest version
VERSION=$(curl -sL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)

if [ -z "$VERSION" ]; then
  echo "❌ Could not determine latest version"
  exit 1
fi

echo "📥 Downloading FlakeCache CLI $VERSION ($BINARY)..."

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY}"
TMP_FILE=$(mktemp)

curl -fsSL "$DOWNLOAD_URL" -o "$TMP_FILE" || {
  echo "❌ Download failed"
  exit 1
}

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
flakecache --version 2>/dev/null || echo "Version: $VERSION"
echo ""
echo "Get started:"
echo "  flakecache login     # Authenticate"
echo "  flakecache push      # Push to cache"
echo "  flakecache --help    # Show all commands"
