#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test "$(uname -s)" = Darwin
cargo build --release --workspace --locked
mkdir -p dist/Stratum.app/Contents/MacOS dist/Stratum.app/Contents/Resources
cp target/release/stratum-desktop dist/Stratum.app/Contents/MacOS/stratum-desktop
cp packaging/Info.plist dist/Stratum.app/Contents/Info.plist
cp target/release/stratum target/release/stratum-tui dist/
cp -R docs dist/
cp README.md dist/
cp LICENSE-MIT LICENSE-APACHE dist/
tar -czf dist/stratum-macos.tar.gz -C dist Stratum.app stratum stratum-tui README.md LICENSE-MIT LICENSE-APACHE docs
shasum -a 256 dist/stratum-macos.tar.gz
