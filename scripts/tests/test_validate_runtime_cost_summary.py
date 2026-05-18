import copy
import math
import tempfile
import unittest
from pathlib import Path

from scripts import validate_runtime_cost_summary


class ValidateRuntimeCostSummaryTests(unittest.TestCase):
    def _valid_summary(self) -> dict:
        metric = {"median": 1.0}
        modes = {
            mode: {
                "throughput_rps": metric,
                "latency_p95_ms": metric,
                "run_requests": metric,
                "run_stages": metric,
                "run_queues": metric,
                "runtime_snapshots": metric,
                "effective_tokio_sampler_config_present_rounds": 1,
                "drop_path_signal_present_rounds": 1,
            }
            for mode in validate_runtime_cost_summary.EXPECTED_MODES
        }
        modes["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 1
        return {
            "absolute_metrics": modes,
            "tracing_vs_native_ratios": {key: 1.0 for key in validate_runtime_cost_summary.REQUIRED_RATIO_KEYS},
        }

    def test_valid_fixture_passes(self) -> None:
        validate_runtime_cost_summary.validate_summary(self._valid_summary())

    def test_missing_mode_fails(self) -> None:
        summary = self._valid_summary()
        del summary["absolute_metrics"]["tracing_light"]
        with self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate_summary(summary)

    def test_missing_tracing_runtime_snapshots_fails(self) -> None:
        summary = self._valid_summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 0.0
        with self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate_summary(summary)

    def test_missing_sampler_metadata_fails(self) -> None:
        summary = self._valid_summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 0
        with self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate_summary(summary)

    def test_missing_drop_path_signal_fails(self) -> None:
        summary = self._valid_summary()
        summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 0
        with self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate_summary(summary)

    def test_non_finite_ratio_fails(self) -> None:
        summary = self._valid_summary()
        summary["tracing_vs_native_ratios"]["tracing_light_vs_core_light_latency_p95"] = math.inf
        with self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate_summary(summary)

    def test_missing_required_ratio_key_fails(self) -> None:
        summary = self._valid_summary()
        del summary["tracing_vs_native_ratios"]["tracing_light_vs_core_light_latency_p95"]
        with self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate_summary(summary)

    def test_raw_jsonl_guard(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            raw = Path(tmp) / "raw.jsonl"
            raw.write_text("\n", encoding="utf-8")
            with self.assertRaises(SystemExit):
                validate_runtime_cost_summary._read_non_empty_jsonl(raw)


if __name__ == "__main__":
    unittest.main()
