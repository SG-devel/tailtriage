#!/usr/bin/env python3
"""Detect drift between regenerated demo analyses and committed fixtures."""

from __future__ import annotations

import argparse
import json
import re
import tempfile
from pathlib import Path

from _demo_runner import (
    PROFILE_CHOICES,
    load_report_json,
    repo_root,
    run_and_analyze,
    variant_paths,
    write_before_after_comparison,
)
from demo_tool import snapshot_blocking, snapshot_downstream, snapshot_queue


class FixtureDriftError(RuntimeError):
    """Raised when one or more committed fixtures differ from regenerated outputs."""


def _read_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8"))


def _write_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def _normalize_text(value: str) -> str:
    return re.sub(r"\d+", "<n>", value)


def _normalize_suspect(suspect: dict) -> dict:
    return {
        "kind": suspect.get("kind"),
    }


def _normalize_analysis(payload: object) -> object:
    if not isinstance(payload, dict):
        return payload

    suspect_kinds = {
        (suspect or {}).get("kind")
        for suspect in [
            payload.get("primary_suspect") or {},
            *(payload.get("secondary_suspects") or []),
        ]
    }
    suspects = [{"kind": kind} for kind in sorted(suspect_kinds, key=str)]

    normalized = {
        "suspects": suspects,
        "warnings": [_normalize_text(str(item)) for item in (payload.get("warnings") or [])],
    }

    if "request_count" in payload:
        normalized["request_count"] = payload["request_count"]

    return normalized


def _normalize_analysis_for_fixture(payload: object, fixture_rel: Path) -> object:
    normalized = _normalize_analysis(payload)
    if (
        isinstance(normalized, dict)
        and "demos/executor_pressure_service/fixtures/" in fixture_rel.as_posix()
    ):
        suspects = normalized.get("suspects")
        if isinstance(suspects, list):
            for suspect in suspects:
                if not isinstance(suspect, dict):
                    continue
                kind = suspect.get("kind")
                if kind in {
                    "executor_pressure_suspected",
                    "application_queue_saturation",
                    "downstream_stage_dominates",
                }:
                    suspect["kind"] = "executor_pressure_family"
            normalized["suspects"] = [{"kind": kind} for kind in sorted({s.get("kind") for s in suspects}, key=str)]
    return normalized


def _run_before_after(
    root_dir: Path,
    demo_manifest: Path,
    temp_artifact_dir: Path,
    snapshot_fn,
    *,
    profile: str = "dev",
) -> None:
    cli_manifest = root_dir / "tailtriage-cli/Cargo.toml"
    for variant in ("before", "after"):
        run_path, analysis_path = variant_paths(temp_artifact_dir, variant)
        mode_arg = "baseline" if variant == "before" else "mitigated"
        run_and_analyze(
            demo_manifest,
            cli_manifest,
            run_path,
            analysis_path,
            mode_arg,
            profile=profile,
        )

    before = load_report_json(temp_artifact_dir / "before-analysis.json")
    after = load_report_json(temp_artifact_dir / "after-analysis.json")
    write_before_after_comparison(temp_artifact_dir, snapshot_fn(before), snapshot_fn(after))


def _scenario_specs() -> list[tuple[Path, Path]]:
    return [
        (Path("demos/queue_service/fixtures/before-analysis.json"), Path("queue/before-analysis.json")),
        (Path("demos/queue_service/fixtures/after-analysis.json"), Path("queue/after-analysis.json")),
        (Path("demos/queue_service/fixtures/sample-analysis.json"), Path("queue/before-analysis.json")),
        (Path("demos/blocking_service/fixtures/before-analysis.json"), Path("blocking/before-analysis.json")),
        (Path("demos/blocking_service/fixtures/after-analysis.json"), Path("blocking/after-analysis.json")),
        (Path("demos/blocking_service/fixtures/sample-analysis.json"), Path("blocking/before-analysis.json")),
        (
            Path("demos/executor_pressure_service/fixtures/before-analysis.json"),
            Path("executor/before-analysis.json"),
        ),
        (
            Path("demos/executor_pressure_service/fixtures/after-analysis.json"),
            Path("executor/after-analysis.json"),
        ),
        (
            Path("demos/executor_pressure_service/fixtures/sample-analysis.json"),
            Path("executor/before-analysis.json"),
        ),
        (
            Path("demos/downstream_service/fixtures/before-analysis.json"),
            Path("downstream/before-analysis.json"),
        ),
        (
            Path("demos/downstream_service/fixtures/after-analysis.json"),
            Path("downstream/after-analysis.json"),
        ),
        (
            Path("demos/downstream_service/fixtures/sample-analysis.json"),
            Path("downstream/before-analysis.json"),
        ),
        (
            Path("demos/downstream_service/fixtures/before-after-comparison.json"),
            Path("downstream/before-after-comparison.json"),
        ),
        (
            Path("demos/mixed_contention_service/fixtures/baseline-analysis.json"),
            Path("mixed/before-analysis.json"),
        ),
        (
            Path("demos/mixed_contention_service/fixtures/mitigated-analysis.json"),
            Path("mixed/after-analysis.json"),
        ),
        (
            Path("demos/cold_start_burst_service/fixtures/before-analysis.json"),
            Path("cold-start/before-analysis.json"),
        ),
        (
            Path("demos/cold_start_burst_service/fixtures/after-analysis.json"),
            Path("cold-start/after-analysis.json"),
        ),
        (
            Path("demos/db_pool_saturation_service/fixtures/before-analysis.json"),
            Path("db-pool/before-analysis.json"),
        ),
        (
            Path("demos/db_pool_saturation_service/fixtures/after-analysis.json"),
            Path("db-pool/after-analysis.json"),
        ),
        (
            Path("demos/shared_state_lock_service/fixtures/before-analysis.json"),
            Path("shared-lock/before-analysis.json"),
        ),
        (
            Path("demos/shared_state_lock_service/fixtures/after-analysis.json"),
            Path("shared-lock/after-analysis.json"),
        ),
        (
            Path("demos/retry_storm_service/fixtures/before-analysis.json"),
            Path("retry-storm/before-analysis.json"),
        ),
        (
            Path("demos/retry_storm_service/fixtures/after-analysis.json"),
            Path("retry-storm/after-analysis.json"),
        ),
    ]


def regenerate_outputs(root_dir: Path, out_dir: Path, *, profile: str = "dev") -> None:
    _run_before_after(
        root_dir,
        root_dir / "demos/queue_service/Cargo.toml",
        out_dir / "queue",
        snapshot_queue,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/blocking_service/Cargo.toml",
        out_dir / "blocking",
        snapshot_blocking,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/executor_pressure_service/Cargo.toml",
        out_dir / "executor",
        snapshot_queue,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/downstream_service/Cargo.toml",
        out_dir / "downstream",
        snapshot_downstream,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/mixed_contention_service/Cargo.toml",
        out_dir / "mixed",
        snapshot_queue,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/cold_start_burst_service/Cargo.toml",
        out_dir / "cold-start",
        snapshot_queue,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/db_pool_saturation_service/Cargo.toml",
        out_dir / "db-pool",
        snapshot_queue,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/shared_state_lock_service/Cargo.toml",
        out_dir / "shared-lock",
        snapshot_queue,
        profile=profile,
    )
    _run_before_after(
        root_dir,
        root_dir / "demos/retry_storm_service/Cargo.toml",
        out_dir / "retry-storm",
        snapshot_queue,
        profile=profile,
    )


def check_or_refresh(root_dir: Path, refresh: bool, *, profile: str = "dev") -> None:
    with tempfile.TemporaryDirectory(prefix="tailtriage-fixture-drift-") as temp_dir:
        generated_root = Path(temp_dir)
        regenerate_outputs(root_dir, generated_root, profile=profile)

        drifted: list[str] = []
        for fixture_rel, generated_rel in _scenario_specs():
            fixture_path = root_dir / fixture_rel
            generated_path = generated_root / generated_rel
            expected = _read_json(generated_path)

            if refresh:
                _write_json(fixture_path, expected)
                continue

            committed = _read_json(fixture_path)
            if _normalize_analysis_for_fixture(
                committed, fixture_rel
            ) != _normalize_analysis_for_fixture(expected, fixture_rel):
                drifted.append(str(fixture_rel))

        if drifted:
            lines = "\n".join(f"- {path}" for path in drifted)
            raise FixtureDriftError(
                "Detected stale demo analysis fixtures:\n"
                f"{lines}\n"
                "Run `python3 scripts/check_demo_fixture_drift.py --refresh` to refresh them."
            )


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Detect or refresh committed demo analysis fixtures.",
    )
    parser.add_argument(
        "--refresh",
        action="store_true",
        help="Rewrite committed fixtures with regenerated outputs.",
    )
    parser.add_argument(
        "--profile",
        choices=PROFILE_CHOICES,
        default="dev",
        help=(
            "Cargo profile for fixture regeneration. "
            "Policy: fixtures are normalized and expected to be stable across profiles; "
            "CI validates this in both dev and release."
        ),
    )
    parser.add_argument(
        "--release",
        action="store_true",
        help="Shortcut for --profile release.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)
    root_dir = repo_root(__file__)
    profile = "release" if args.release else args.profile
    check_or_refresh(root_dir, refresh=args.refresh, profile=profile)
    if args.refresh:
        print(f"demo analysis fixtures refreshed (profile={profile})")
    else:
        print(f"demo analysis fixtures are up to date (profile={profile})")


if __name__ == "__main__":
    main()
