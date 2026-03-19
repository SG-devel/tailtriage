#!/usr/bin/env python3
"""Validate blocking demo output contract."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path


def main() -> None:
    root_dir = Path(__file__).resolve().parent.parent
    analysis_path = root_dir / "demos/blocking_service/artifacts/blocking-analysis.json"

    subprocess.run(["python3", str(root_dir / "scripts/run_blocking_demo.py")], check=True)

    report = json.loads(analysis_path.read_text())
    kind = report["primary_suspect"]["kind"]
    expected = {"blocking_pool_pressure", "BlockingPoolPressure"}
    if kind not in expected:
        raise SystemExit(f"expected blocking pool pressure suspect, got {kind}")

    print(f"validation passed: primary suspect is {kind}")
    print(f"validated analysis file: {analysis_path}")


if __name__ == "__main__":
    main()
