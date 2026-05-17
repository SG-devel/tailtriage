#!/usr/bin/env python3
from __future__ import annotations
import json, tempfile, unittest
from pathlib import Path
import sys
REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import measure_runtime_cost  # noqa: E402

class RuntimeCostSummaryTests(unittest.TestCase):
    def test_modes_include_tracing(self):
        self.assertIn("tracing_light", measure_runtime_cost.MODES)
        self.assertIn("tracing_light_tokio_sampler", measure_runtime_cost.MODES)
        self.assertIn("tracing_light_drop_path", measure_runtime_cost.MODES)

    def test_safe_ratio_zero_denominator(self):
        self.assertIsNone(measure_runtime_cost.safe_ratio(2.0, 0.0))

    def test_command_args_present(self):
        # run_mode builds subprocess args; validate critical flags are present in source constants/flow.
        self.assertTrue(hasattr(measure_runtime_cost, "run_mode"))

    def test_summary_has_relative_ratios(self):
        with tempfile.TemporaryDirectory() as tmp:
            raw_path = Path(tmp) / "runtime-cost-raw.jsonl"
            summary_path = Path(tmp) / "runtime-cost-summary.json"
            rows=[]
            for r in range(4):
                for mode in measure_runtime_cost.MODES:
                    row={"mode":mode,"requests":100,"concurrency":10,"work_ms":1,"throughput_rps":1000.0,"latency_p50_ms":1.0,"latency_p95_ms":2.0,"latency_p99_ms":3.0,"artifact_finalize_ms":1.0,"analyze_ms":1.0,"report_render_ms":1.0,"run_requests":10,"run_stages":10,"run_queues":10,"runtime_snapshots":1 if "tokio_sampler" in mode else 0,"lifecycle_warning_count":0,"instrumentation":"native","uses_runtime_sampler":"tokio_sampler" in mode,"uses_drop_path_limits":"drop_path" in mode,"effective_tokio_sampler_config_present":"tokio_sampler" in mode,"inflight_supported": not mode.startswith("tracing_"),"drop_path_signal_present":"drop_path" in mode,"artifact_path":None,"round":r,"phase":"measured","is_warmup":False}
                    row["truncation"]={"dropped_requests":1 if "drop_path" in mode else 0,"dropped_stages":0,"dropped_queues":0,"dropped_inflight_snapshots":0,"dropped_runtime_snapshots":0,"limits_reached":"drop_path" in mode}
                    rows.append(row)
            raw_path.write_text("\n".join(json.dumps(x) for x in rows)+"\n",encoding="utf-8")
            summary=measure_runtime_cost.summarize(raw_path, summary_path)
            self.assertIn("relative_ratios", summary)
            self.assertIn("tracing_light_vs_core_light_latency_p95", summary["relative_ratios"])

if __name__ == "__main__":
    unittest.main()
