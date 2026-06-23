#!/usr/bin/env bash
# Build the zjstatus wasm, then run the {claude_status} integration scenarios
# against a real headless zellij session (hosted in tmux).
#
# Requires: tmux, zellij, rust + wasm32-wasip1 target, python3.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$(cd "$HERE/../.." && pwd)"

echo "=== building zjstatus wasm (release) ==="
( cd "$CRATE_DIR" && cargo build --release )

echo "=== installing wasm (standard path => cached permission applies) ==="
mkdir -p "$HOME/.config/zellij/plugins"
cp "$CRATE_DIR/target/wasm32-wasip1/release/zjstatus.wasm" \
   "$HOME/.config/zellij/plugins/zjstatus.wasm"

echo "=== running integration scenarios ==="
cd "$HERE"
exec python3 scenarios.py "$@"
