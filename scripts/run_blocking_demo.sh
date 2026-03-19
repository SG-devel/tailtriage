#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_PATH="${1:-$ROOT_DIR/demos/blocking_service/artifacts/blocking-run.json}"
ANALYSIS_PATH="$ROOT_DIR/demos/blocking_service/artifacts/blocking-analysis.json"

mkdir -p "$(dirname "$ARTIFACT_PATH")"

cargo run --quiet --manifest-path "$ROOT_DIR/demos/blocking_service/Cargo.toml" -- "$ARTIFACT_PATH"

cargo run --quiet --manifest-path "$ROOT_DIR/tailscope-cli/Cargo.toml" -- analyze "$ARTIFACT_PATH" --format json \
  > "$ANALYSIS_PATH"

printf 'run artifact: %s\n' "$ARTIFACT_PATH"
printf 'analysis: %s\n' "$ANALYSIS_PATH"
