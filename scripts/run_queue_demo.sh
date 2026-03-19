#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_PATH="${1:-$ROOT_DIR/demos/queue_service/artifacts/queue-run.json}"

mkdir -p "$(dirname "$ARTIFACT_PATH")"

cargo run --quiet --manifest-path "$ROOT_DIR/demos/queue_service/Cargo.toml" -- "$ARTIFACT_PATH"

cargo run --quiet --manifest-path "$ROOT_DIR/tailscope-cli/Cargo.toml" -- analyze "$ARTIFACT_PATH" --format json \
  > "$ROOT_DIR/demos/queue_service/artifacts/queue-analysis.json"

printf 'run artifact: %s\n' "$ARTIFACT_PATH"
printf 'analysis: %s\n' "$ROOT_DIR/demos/queue_service/artifacts/queue-analysis.json"
