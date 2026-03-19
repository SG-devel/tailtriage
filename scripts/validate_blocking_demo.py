#!/usr/bin/env python3
"""Validate blocking demo output contract."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path


EXPECTED_BLOCKING_KIND = {"blocking_pool_pressure", "BlockingPoolPressure"}


def extract_blocking_queue_depth_p95(report: dict) -> int | None:
    suspect = report.get("primary_suspect") or {}
    for evidence in suspect.get("evidence") or []:
        match = re.search(r"Blocking queue depth p95 is (\d+)", evidence)
        if match:
            return int(match.group(1))
    return None


def main() -> None:
    root_dir = Path(__file__).resolve().parent.parent
    artifact_dir = root_dir / "demos/blocking_service/artifacts"
    before_analysis_path = artifact_dir / "before-analysis.json"
    after_analysis_path = artifact_dir / "after-analysis.json"

    subprocess.run(["python3", str(root_dir / "scripts/run_blocking_demo.py")], check=True)

    before = json.loads(before_analysis_path.read_text())
    after = json.loads(after_analysis_path.read_text())

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_BLOCKING_KIND:
        raise SystemExit(f"expected blocking pool pressure suspect in baseline, got {before_kind}")

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]

    before_service_share = before.get("p95_service_share_permille")
    after_service_share = after.get("p95_service_share_permille")

    before_blocking_depth = extract_blocking_queue_depth_p95(before)
    after_blocking_depth = extract_blocking_queue_depth_p95(after)

    improvement_signals = []
    if after_score < before_score:
        improvement_signals.append("score")

    if (
        before_service_share is not None
        and after_service_share is not None
        and after_service_share < before_service_share
    ):
        improvement_signals.append("service_share")

    if (
        before_blocking_depth is not None
        and after_blocking_depth is not None
        and after_blocking_depth < before_blocking_depth
    ):
        improvement_signals.append("blocking_queue_depth")

    if not improvement_signals:
        raise SystemExit(
            "expected at least one non-latency improvement signal (score/share/blocking depth), "
            f"got score {before_score}->{after_score}, "
            f"service_share {before_service_share}->{after_service_share}, "
            f"blocking_queue_depth {before_blocking_depth}->{after_blocking_depth}"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}, "
        "service-share {} -> {}, blocking-depth {} -> {} (signals: {})".format(
            before_kind,
            before_p95,
            after_p95,
            before_score,
            after_score,
            before_service_share,
            after_service_share,
            before_blocking_depth,
            after_blocking_depth,
            ", ".join(improvement_signals),
        )
    )
    print(f"validated analysis files: {before_analysis_path}, {after_analysis_path}")


if __name__ == "__main__":
    main()
