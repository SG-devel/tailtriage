import json
from pathlib import Path
import tempfile
import unittest

from scripts import analyzer_numeric_sensitivity as sensitivity


class AnalyzerNumericSensitivityTests(unittest.TestCase):
    def assert_recorded_plan_rejected(self, mutate):
        (sensitivity.REPO / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=sensitivity.REPO / "target") as directory:
            output = Path(directory)
            experiments, inputs = sensitivity.write_plan(output)
            mutate(output, experiments, inputs)
            with self.assertRaises(sensitivity.VerificationError):
                sensitivity.verify_recorded_plan(output, experiments, inputs)

    # TT-TEST: support
    def test_plan_has_stable_unique_matrix(self):
        experiments, inputs = sensitivity.generate_plan()
        self.assertEqual(len(experiments), 108)
        self.assertEqual(len({item["experiment_id"] for item in experiments}), 108)
        self.assertTrue({item["input_id"] for item in experiments} <= inputs.keys())
        counts = {}
        for item in experiments:
            counts[item["family"]] = counts.get(item["family"], 0) + 1
        self.assertEqual(counts, {
            "p95": 3, "evidence": 3, "sample_quality": 8, "queue_trigger": 24,
            "queue_extreme": 5, "blocking": 2, "executor": 10,
            "executor_trigger": 3, "downstream": 18, "downstream_extreme": 2,
            "confidence": 15, "temporal_count": 9, "temporal_share": 3,
            "temporal_p95": 3,
        })

    # TT-TEST: support
    def test_generated_geometry_is_structurally_correct(self):
        _, inputs = sensitivity.generate_plan()
        for n in (19, 20, 21):
            run = inputs[f"p95-{n}"]
            self.assertEqual(len(run["requests"]), n)
            self.assertEqual(sorted(q["wait_us"] for q in run["queues"]), [100] * (n - 1) + [900])
        for n in (7, 8, 19, 20, 39, 40, 99, 100):
            self.assertEqual(len(inputs[f"sample-{n:03d}"]["requests"]), n)
        for target in (499, 500, 999, 1000, 1999, 2000, 3999, 4000, 7999, 8000):
            snapshot = inputs[f"exec-{target}"]["runtime_snapshots"][0]
            self.assertEqual((snapshot["global_queue_depth"], snapshot["local_queue_depth"],
                              snapshot["worker_count"]), (target, 0, 1000))
        for k in (2, 3, 4, 5):
            self.assertEqual(len(inputs[f"down-k{k}-extreme"]["stages"]), k)
        for n in (19, 20, 21):
            run = inputs[f"temp-n{n}"]
            self.assertEqual(len(run["requests"]), n)
            self.assertEqual(sum(r["latency_us"] == 1000 for r in run["requests"]), n // 2)

    # TT-TEST: support
    def test_plan_bytes_are_stable_and_runs_are_complete_schema_v2(self):
        (sensitivity.REPO / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=sensitivity.REPO / "target") as first_dir, \
             tempfile.TemporaryDirectory(dir=sensitivity.REPO / "target") as second_dir:
            first = Path(first_dir); second = Path(second_dir)
            sensitivity.write_plan(first); sensitivity.write_plan(second)
            self.assertEqual((first / "experiment-plan.csv").read_bytes(),
                             (second / "experiment-plan.csv").read_bytes())
            first_inputs = sorted((first / "inputs").iterdir())
            self.assertEqual([path.name for path in first_inputs],
                             [path.name for path in sorted((second / "inputs").iterdir())])
            for path in first_inputs:
                run = json.loads(path.read_bytes())
                self.assertEqual(run["schema_version"], 2)
                self.assertFalse(run["truncation"]["limits_hit"])
                self.assertTrue(all(value == 0 for key, value in run["truncation"].items()
                                    if key.startswith("dropped_")))
                self.assertEqual(path.read_bytes(), (second / "inputs" / path.name).read_bytes())

    # TT-TEST: support
    def test_output_must_stay_under_repository(self):
        inside = sensitivity.ensure_output(sensitivity.REPO / "target" / "custom")
        self.assertTrue(inside.is_relative_to(sensitivity.REPO.resolve()))
        with self.assertRaises(SystemExit):
            sensitivity.ensure_output(Path(tempfile.gettempdir()) / "outside-tailtriage")

    # TT-TEST: support
    def test_recorded_plan_and_inputs_verify(self):
        (sensitivity.REPO / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=sensitivity.REPO / "target") as directory:
            output = Path(directory)
            experiments, inputs = sensitivity.write_plan(output)
            sensitivity.verify_recorded_plan(output, experiments, inputs)

    # TT-TEST: support
    def test_mutated_recorded_plan_is_rejected(self):
        def mutate(output, _experiments, _inputs):
            path = output / "experiment-plan.csv"
            path.write_bytes(path.read_bytes().replace(b"P95-19", b"P95-X9", 1))
        self.assert_recorded_plan_rejected(mutate)

    # TT-TEST: support
    def test_mutated_recorded_input_is_rejected(self):
        def mutate(output, _experiments, _inputs):
            path = output / "inputs" / "p95-19.json"
            path.write_bytes(path.read_bytes().replace(b'"latency_us":1000',
                                                       b'"latency_us":1001', 1))
        self.assert_recorded_plan_rejected(mutate)

    # TT-TEST: support
    def test_missing_recorded_input_is_rejected(self):
        def mutate(output, _experiments, _inputs):
            (output / "inputs" / "p95-19.json").unlink()
        self.assert_recorded_plan_rejected(mutate)

    # TT-TEST: support
    def test_extra_recorded_input_is_rejected(self):
        def mutate(output, _experiments, _inputs):
            (output / "inputs" / "stale.json").write_text("{}\n", encoding="utf-8")
        self.assert_recorded_plan_rejected(mutate)

    # TT-TEST: support
    def test_current_generator_input_drift_is_rejected(self):
        def mutate(_output, _experiments, inputs):
            inputs["p95-19"]["requests"][0]["latency_us"] += 1
        self.assert_recorded_plan_rejected(mutate)


if __name__ == "__main__":
    unittest.main()
