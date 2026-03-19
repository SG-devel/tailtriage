#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_DIR="${1:-$ROOT_DIR/demos/runtime_cost/artifacts}"
REQUESTS="${REQUESTS:-1200}"
CONCURRENCY="${CONCURRENCY:-48}"
WORK_MS="${WORK_MS:-3}"
ITERATIONS="${ITERATIONS:-5}"

mkdir -p "$ARTIFACT_DIR"
RAW_PATH="$ARTIFACT_DIR/runtime-cost-raw.jsonl"
SUMMARY_PATH="$ARTIFACT_DIR/runtime-cost-summary.json"
: > "$RAW_PATH"

for mode in baseline light investigation; do
  for ((i=1; i<=ITERATIONS; i++)); do
    cargo run --quiet --manifest-path "$ROOT_DIR/demos/runtime_cost/Cargo.toml" -- \
      --mode "$mode" \
      --requests "$REQUESTS" \
      --concurrency "$CONCURRENCY" \
      --work-ms "$WORK_MS" \
      --output-dir "$ARTIFACT_DIR" >> "$RAW_PATH"
  done
done

python3 - "$RAW_PATH" "$SUMMARY_PATH" <<'PY'
import json
import statistics
import sys

raw_path = sys.argv[1]
out_path = sys.argv[2]

rows = [json.loads(line) for line in open(raw_path, encoding='utf-8') if line.strip()]
by_mode = {}
for row in rows:
    by_mode.setdefault(row["mode"], []).append(row)

required = ["baseline", "light", "investigation"]
for mode in required:
    if mode not in by_mode:
        raise SystemExit(f"missing mode: {mode}")

summary = {
    "requests": by_mode["baseline"][0]["requests"],
    "concurrency": by_mode["baseline"][0]["concurrency"],
    "work_ms": by_mode["baseline"][0]["work_ms"],
    "iterations_per_mode": len(by_mode["baseline"]),
    "modes": {},
}

for mode, values in by_mode.items():
    metrics = {k: [row[k] for row in values] for k in ["throughput_rps", "latency_p50_ms", "latency_p95_ms", "latency_p99_ms"]}
    summary["modes"][mode] = {
        "throughput_rps_mean": statistics.fmean(metrics["throughput_rps"]),
        "latency_p50_ms_mean": statistics.fmean(metrics["latency_p50_ms"]),
        "latency_p95_ms_mean": statistics.fmean(metrics["latency_p95_ms"]),
        "latency_p99_ms_mean": statistics.fmean(metrics["latency_p99_ms"]),
    }

baseline = summary["modes"]["baseline"]
for mode in ["light", "investigation"]:
    m = summary["modes"][mode]
    m["throughput_overhead_pct_vs_baseline"] = ((baseline["throughput_rps_mean"] - m["throughput_rps_mean"]) / baseline["throughput_rps_mean"]) * 100.0
    m["p50_overhead_pct_vs_baseline"] = ((m["latency_p50_ms_mean"] - baseline["latency_p50_ms_mean"]) / baseline["latency_p50_ms_mean"]) * 100.0
    m["p95_overhead_pct_vs_baseline"] = ((m["latency_p95_ms_mean"] - baseline["latency_p95_ms_mean"]) / baseline["latency_p95_ms_mean"]) * 100.0
    m["p99_overhead_pct_vs_baseline"] = ((m["latency_p99_ms_mean"] - baseline["latency_p99_ms_mean"]) / baseline["latency_p99_ms_mean"]) * 100.0

with open(out_path, "w", encoding="utf-8") as f:
    json.dump(summary, f, indent=2)

print(json.dumps(summary, indent=2))
PY

echo "raw results: $RAW_PATH"
echo "summary: $SUMMARY_PATH"
