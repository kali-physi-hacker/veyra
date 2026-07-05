#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo run --locked -p stratum-cli -- api openapi --json > docs/openapi.json
