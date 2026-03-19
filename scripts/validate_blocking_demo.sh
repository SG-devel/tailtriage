#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANALYSIS_PATH="$ROOT_DIR/demos/blocking_service/artifacts/blocking-analysis.json"

"$ROOT_DIR/scripts/run_blocking_demo.sh"

python3 - <<'PY'
import json
from pathlib import Path

analysis_path = Path("demos/blocking_service/artifacts/blocking-analysis.json")
report = json.loads(analysis_path.read_text())

kind = report["primary_suspect"]["kind"]
expected = {"blocking_pool_pressure", "BlockingPoolPressure"}
if kind not in expected:
    raise SystemExit(f"expected blocking pool pressure suspect, got {kind}")

print(f"validation passed: primary suspect is {kind}")
PY

echo "validated analysis file: $ANALYSIS_PATH"
