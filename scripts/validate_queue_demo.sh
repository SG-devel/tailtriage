#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BEFORE_ANALYSIS_PATH="$ROOT_DIR/demos/queue_service/artifacts/before-analysis.json"
AFTER_ANALYSIS_PATH="$ROOT_DIR/demos/queue_service/artifacts/after-analysis.json"

"$ROOT_DIR/scripts/run_queue_demo.sh"

python3 - <<'PY'
import json
from pathlib import Path

before_analysis_path = Path("demos/queue_service/artifacts/before-analysis.json")
after_analysis_path = Path("demos/queue_service/artifacts/after-analysis.json")

before = json.loads(before_analysis_path.read_text())
after = json.loads(after_analysis_path.read_text())

kind = before["primary_suspect"]["kind"]
expected = {"application_queue_saturation", "ApplicationQueueSaturation"}
if kind not in expected:
    raise SystemExit(f"expected queue saturation suspect in baseline, got {kind}")

before_p95 = before["p95_latency_us"]
after_p95 = after["p95_latency_us"]
before_score = before["primary_suspect"]["score"]
after_score = after["primary_suspect"]["score"]

if after_p95 >= before_p95:
    raise SystemExit(
        f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
    )

if after_score >= before_score:
    raise SystemExit(
        f"expected mitigated suspect score to drop, got before={before_score} after={after_score}"
    )

print(
    "validation passed: baseline suspect kind={}, p95 {}us -> {}us, "
    "score {} -> {}".format(kind, before_p95, after_p95, before_score, after_score)
)
PY

echo "validated analysis files: $BEFORE_ANALYSIS_PATH, $AFTER_ANALYSIS_PATH"
