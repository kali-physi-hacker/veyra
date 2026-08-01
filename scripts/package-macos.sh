#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test "$(uname -s)" = Darwin
release_dir=${1:-dist}
case "$release_dir" in
  dist|dist-preview) ;;
  *) echo "Usage: package-macos.sh [dist|dist-preview]" >&2; exit 1 ;;
esac
cargo build --release --workspace --locked
mkdir -p "$release_dir/Stratum.app/Contents/MacOS" "$release_dir/Stratum.app/Contents/Resources"
cp target/release/stratum-desktop "$release_dir/Stratum.app/Contents/MacOS/stratum-desktop"
cp packaging/Info.plist "$release_dir/Stratum.app/Contents/Info.plist"
cp target/release/stratum target/release/stratum-tui "$release_dir/"
cp -R docs "$release_dir/"
cp README.md LICENSE-MIT LICENSE-APACHE "$release_dir/"
tar -czf "$release_dir/stratum-macos.tar.gz" -C "$release_dir" Stratum.app stratum stratum-tui README.md LICENSE-MIT LICENSE-APACHE docs
shasum -a 256 "$release_dir/stratum-macos.tar.gz"
