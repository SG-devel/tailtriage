#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANALYSIS_PATH="$ROOT_DIR/demos/queue_service/artifacts/queue-analysis.json"

"$ROOT_DIR/scripts/run_queue_demo.sh"

python3 - <<'PY'
import json
from pathlib import Path

analysis_path = Path("demos/queue_service/artifacts/queue-analysis.json")
report = json.loads(analysis_path.read_text())

kind = report["primary_suspect"]["kind"]
expected = {"application_queue_saturation", "ApplicationQueueSaturation"}
if kind not in expected:
    raise SystemExit(f"expected queue saturation suspect, got {kind}")

print(f"validation passed: primary suspect is {kind}")
PY

echo "validated analysis file: $ANALYSIS_PATH"
