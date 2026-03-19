#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_DIR="$ROOT_DIR/demos/queue_service/artifacts"
MODE="${1:-both}"

run_variant() {
  local variant="$1"
  local run_path="$ARTIFACT_DIR/${variant}-run.json"
  local analysis_path="$ARTIFACT_DIR/${variant}-analysis.json"
  local mode_arg="$variant"

  if [[ "$variant" == "before" ]]; then
    mode_arg="baseline"
  else
    mode_arg="mitigated"
  fi

  mkdir -p "$ARTIFACT_DIR"

  cargo run --quiet --manifest-path "$ROOT_DIR/demos/queue_service/Cargo.toml" -- "$run_path" "$mode_arg"
  cargo run --quiet --manifest-path "$ROOT_DIR/tailscope-cli/Cargo.toml" -- analyze "$run_path" --format json \
    > "$analysis_path"

  printf 'run artifact (%s): %s\n' "$variant" "$run_path"
  printf 'analysis (%s): %s\n' "$variant" "$analysis_path"
}

case "$MODE" in
  before|baseline)
    run_variant before
    ;;
  after|mitigated)
    run_variant after
    ;;
  both)
    run_variant before
    run_variant after
    ;;
  *)
    echo "unsupported mode '$MODE'; expected one of: before, after, both, baseline, mitigated" >&2
    exit 1
    ;;
esac

if [[ "$MODE" == "both" ]]; then
  python3 - <<'PY'
import json
from pathlib import Path

artifacts = Path("demos/queue_service/artifacts")
before = json.loads((artifacts / "before-analysis.json").read_text())
after = json.loads((artifacts / "after-analysis.json").read_text())

comparison = {
    "before": {
        "primary_suspect_kind": before["primary_suspect"]["kind"],
        "primary_suspect_score": before["primary_suspect"]["score"],
        "p95_latency_us": before["p95_latency_us"],
        "p95_queue_share_permille": before.get("p95_queue_share_permille"),
    },
    "after": {
        "primary_suspect_kind": after["primary_suspect"]["kind"],
        "primary_suspect_score": after["primary_suspect"]["score"],
        "p95_latency_us": after["p95_latency_us"],
        "p95_queue_share_permille": after.get("p95_queue_share_permille"),
    },
}

comparison["delta"] = {
    "p95_latency_us": comparison["after"]["p95_latency_us"] - comparison["before"]["p95_latency_us"],
    "primary_suspect_score": comparison["after"]["primary_suspect_score"]
    - comparison["before"]["primary_suspect_score"],
    "p95_queue_share_permille": (
        None
        if comparison["before"]["p95_queue_share_permille"] is None
        or comparison["after"]["p95_queue_share_permille"] is None
        else comparison["after"]["p95_queue_share_permille"]
        - comparison["before"]["p95_queue_share_permille"]
    ),
}

comparison_path = artifacts / "before-after-comparison.json"
comparison_path.write_text(json.dumps(comparison, indent=2) + "\n")
print(f"comparison: {comparison_path}")
PY
fi
