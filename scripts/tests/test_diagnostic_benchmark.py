#!/usr/bin/env python3
from __future__ import annotations

import sys
import json
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import diagnostic_benchmark


class DiagnosticBenchmarkTests(unittest.TestCase):
    def _write(self, root: Path, rel: str, data: dict):
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(data), encoding="utf-8")

    def _manifest(self, cases):
        return {"version": 1, "cases": cases}

    def test_manifest_rejects_missing_fields(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "m.json"
            p.write_text(json.dumps({"cases": [{"id": "x"}]}), encoding="utf-8")
            with self.assertRaises(SystemExit):
                diagnostic_benchmark.load_manifest(p)

    def test_duplicate_and_unknown_gt(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "m.json"
            case = {"id":"a","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"unknown","acceptable_top2":["unknown"],"tags":[],"must_include_evidence":[],"allowed_warnings":[],"notes":"n"}
            p.write_text(json.dumps(self._manifest([case, dict(case)])), encoding="utf-8")
            with self.assertRaises(SystemExit):
                diagnostic_benchmark.load_manifest(p)

    def test_metrics_and_output_shape(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            report = {"primary_suspect":{"kind":"application_queue_saturation","score":80,"confidence":"high","evidence":["Queue wait at p95"],"next_checks":[]},"secondary_suspects":[{"kind":"downstream_stage_dominates","score":50,"confidence":"low","evidence":["Stage x"],"next_checks":[]}],"warnings":[]}
            self._write(root, "a.json", report)
            m = self._manifest([{"id":"c1","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","acceptable_top2":["application_queue_saturation"],"tags":[],"must_include_evidence":["Stage"],"allowed_warnings":[],"notes":"n"}])
            mp = root / "validation/diagnostics/manifest.json"
            mp.parent.mkdir(parents=True)
            mp.write_text(json.dumps(m), encoding="utf-8")
            out = diagnostic_benchmark.run(mp)
            self.assertEqual(out["total_cases"], 1)
            self.assertEqual(out["top1_accuracy"], 1.0)
            self.assertEqual(out["top2_recall"], 1.0)
            self.assertIn("high", out["confidence_bucket_accuracy"])
            self.assertEqual(out["required_evidence_pass_rate"], 1.0)

    def test_warnings_and_threshold_failure(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            report = {"primary_suspect":{"kind":"downstream_stage_dominates","score":70,"confidence":"medium","evidence":["Stage slow"],"next_checks":[]},"secondary_suspects":[],"warnings":["unexpected warning"]}
            self._write(root, "a.json", report)
            m = self._manifest([{"id":"c1","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"downstream_stage_dominates","acceptable_top2":["downstream_stage_dominates"],"tags":[],"must_include_evidence":["Stage"],"allowed_warnings":["known"],"notes":"n"}])
            mp = root / "validation/diagnostics/manifest.json"
            mp.parent.mkdir(parents=True)
            mp.write_text(json.dumps(m), encoding="utf-8")
            out = diagnostic_benchmark.run(mp)
            self.assertEqual(out["unexpected_warning_count"], 1)
            self.assertTrue(out["failed_cases"])


if __name__ == "__main__":
    unittest.main()
