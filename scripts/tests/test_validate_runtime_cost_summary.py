#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import scripts.validate_runtime_cost_summary as validator


def _valid_summary() -> dict:
    modes = {}
    for mode in validator.EXPECTED_MODES:
        modes[mode] = {
            "run_requests": {"median": 1},
            "run_stages": {"median": 1},
            "run_queues": {"median": 1},
            "runtime_snapshots": {"median": 0},
            "effective_tokio_sampler_config_present_rounds": 0,
            "drop_path_signal_present_rounds": 0,
        }
    modes["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 1
    modes["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 1
    modes["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 1

    ratios = {key: 1.0 for key in validator.REQUIRED_RATIO_KEYS}
    return {"absolute_metrics": modes, "tracing_vs_native_ratios": ratios}


class ValidateRuntimeCostSummaryTests(unittest.TestCase):
    def _write(self, summary: dict, raw_content: str = '{"ok":true}\n') -> tuple[Path, Path, tempfile.TemporaryDirectory]:
        tmp = tempfile.TemporaryDirectory()
        raw = Path(tmp.name) / "runtime-cost-raw.jsonl"
        out = Path(tmp.name) / "runtime-cost-summary.json"
        raw.write_text(raw_content, encoding="utf-8")
        out.write_text(json.dumps(summary), encoding="utf-8")
        return raw, out, tmp

    def test_valid_fixture_passes(self) -> None:
        raw, out, tmp = self._write(_valid_summary())
        with tmp:
            validator.validate(raw, out)

    def test_missing_mode_fails(self) -> None:
        summary = _valid_summary()
        del summary["absolute_metrics"]["baseline"]
        raw, out, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validator.validate(raw, out)

    def test_missing_tracing_runtime_snapshots_fails(self) -> None:
        summary = _valid_summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 0
        raw, out, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validator.validate(raw, out)

    def test_missing_sampler_metadata_fails(self) -> None:
        summary = _valid_summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 0
        raw, out, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validator.validate(raw, out)

    def test_missing_drop_path_signal_fails(self) -> None:
        summary = _valid_summary()
        summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 0
        raw, out, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validator.validate(raw, out)

    def test_nan_ratio_fails(self) -> None:
        summary = _valid_summary()
        summary["tracing_vs_native_ratios"]["tracing_light_vs_core_light_latency_p95"] = float("nan")
        raw, out, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validator.validate(raw, out)

    def test_missing_required_ratio_key_fails(self) -> None:
        summary = _valid_summary()
        del summary["tracing_vs_native_ratios"]["tracing_light_vs_core_light_latency_p95"]
        raw, out, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validator.validate(raw, out)


if __name__ == "__main__":
    unittest.main()
