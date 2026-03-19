#!/usr/bin/env python3
"""Validate downstream demo output contract."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path


def main() -> None:
    root_dir = Path(__file__).resolve().parent.parent
    analysis_path = root_dir / "demos/downstream_service/artifacts/downstream-analysis.json"

    subprocess.run(["python3", str(root_dir / "scripts/run_downstream_demo.py")], check=True)

    report = json.loads(analysis_path.read_text(encoding="utf-8"))
    kind = report["primary_suspect"]["kind"]
    expected = {"downstream_stage_dominates", "DownstreamStageDominates"}
    if kind not in expected:
        raise SystemExit(f"expected downstream stage suspect, got {kind}")

    print(f"validation passed: primary suspect is {kind}")
    print(f"validated analysis file: {analysis_path}")


if __name__ == "__main__":
    main()
