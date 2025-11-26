#!/bin/bash

# Fix documentation backticks
sed -i 's/FlakeCache CLI/`FlakeCache` CLI/g' src/error.rs
sed -i 's/Result</`Result`</g' src/error.rs
sed -i 's/Nix /`Nix` /g' src/error.rs

# Fix missing #[must_use] attributes
sed -i '/pub fn exit_code/i\    #[must_use]' src/error.rs
sed -i '/pub fn is_retryable/i\    #[must_use]' src/error.rs
sed -i '/pub fn is_authenticated/i\    #[must_use]' src/config/auth.rs
sed -i '/pub fn is_expired/i\    #[must_use]' src/config/auth.rs
sed -i '/pub fn needs_refresh/i\    #[must_use]' src/config/auth.rs

# Fix duplicate additions (remove duplicates)
sed -i '/^    #[must_use]$/N;s/^\(    #\[must_use\]\)\n\1$/\1/' src/error.rs
sed -i '/^    #[must_use]$/N;s/^\(    #\[must_use\]\)\n\1$/\1/' src/config/auth.rs

echo "✅ Fixed Clippy violations"
