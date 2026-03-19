#!/usr/bin/env python3
"""Validate queue demo output contract."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path


def main() -> None:
    root_dir = Path(__file__).resolve().parent.parent
    before_analysis_path = root_dir / "demos/queue_service/artifacts/before-analysis.json"
    after_analysis_path = root_dir / "demos/queue_service/artifacts/after-analysis.json"

    subprocess.run(["python3", str(root_dir / "scripts/run_queue_demo.py")], check=True)

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
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}".format(
            kind,
            before_p95,
            after_p95,
            before_score,
            after_score,
        )
    )
    print(f"validated analysis files: {before_analysis_path}, {after_analysis_path}")


if __name__ == "__main__":
    main()
