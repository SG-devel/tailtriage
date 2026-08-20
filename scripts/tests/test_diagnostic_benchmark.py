import json, tempfile, unittest
from pathlib import Path
from unittest import mock
from scripts import diagnostic_benchmark as db


def report(kind="application_queue_saturation", conf="high", secondary=None):
    return {
        "primary_suspect": {
            "kind": kind,
            "confidence": conf,
            "score": 1,
            "evidence": ["queue evidence"],
            "next_checks": ["check queue"],
        },
        "secondary_suspects": (
            []
            if secondary is None
            else [
                {
                    "kind": secondary,
                    "confidence": "medium",
                    "evidence": [],
                    "next_checks": [],
                }
            ]
        ),
        "warnings": [],
    }


def case(cid="run", typ="run_artifact", eligible=True, **kw):
    c = {
        "id": cid,
        "artifact": cid + ".json",
        "artifact_type": typ,
        "validation_class": db.TYPE_CLASS[typ],
        "accuracy_eligible": eligible,
        "tags": [],
        "notes": "test case",
        "expected_primary_kinds": ["application_queue_saturation"],
        "required_visible_suspects": ["application_queue_saturation"],
        "must_include_evidence": ["queue"],
        "must_include_next_checks": ["check"],
        "expected_warnings": [],
        "allowed_warnings": [],
    }
    if eligible:
        c.update(
            observation_id=cid,
            ground_truth="application_queue_saturation",
            exact_primary_kind="application_queue_saturation",
        )
    c.update(kw)
    return c


def manifest(*cases):
    return {"schema_version": 2, "cases": list(cases)}


# TT-INVARIANT: D01 primary
class Result:
    def __init__(self, code=0, out="", err=""):
        self.returncode = code
        self.stdout = out
        self.stderr = err


class Tests(unittest.TestCase):
    def write(self, root, cases, reports=None):
        root = Path(root)
        for c, r in zip(cases, reports or [report()] * len(cases)):
            (root / c["artifact"]).write_text(json.dumps(r))
        p = root / "manifest.json"
        p.write_text(json.dumps(manifest(*cases)))
        return p

    def runmock(self, p, outputs):
        with mock.patch.object(db, "_invoke", side_effect=outputs) as inv:
            return db.run(p, 0, 0, 99), inv

    def test_report_only_manifest_fails_with_zero_analyzer_executions(self):
        c = case("r", "analysis_report", False)
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [c])
            (m, f), inv = self.runmock(p, [])
        self.assertEqual(m["report_contract"]["passed_count"], 1)
        self.assertEqual(m["analyzer_execution"]["case_count"], 0)
        self.assertIsNone(m["analyzer_accuracy"]["top1_accuracy"])
        self.assertIn("diagnostic corpus contains zero analyzer-executed cases", f)
        inv.assert_not_called()

    def test_analyzer_execution_without_accuracy_observations_fails_distinctly(self):
        c = case(eligible=False)
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [c])
            (m, f), inv = self.runmock(p, [Result(out=json.dumps(report()))])
        self.assertEqual(m["analyzer_execution"]["case_count"], 1)
        self.assertIn(
            "diagnostic corpus contains zero accuracy-eligible analyzer observations", f
        )
        self.assertNotIn("diagnostic corpus contains zero analyzer-executed cases", f)
        inv.assert_called_once()

    def test_report_contract_results_do_not_change_analyzer_accuracy(self):
        a = case()
        r = case("r", "analysis_report", False)
        vals = []
        for rr in [
            report(),
            report("blocking_pool_pressure", "low", "downstream_stage_dominates"),
        ]:
            with tempfile.TemporaryDirectory() as td:
                p = self.write(td, [a, r], [report(), rr])
                (m, _), _ = self.runmock(p, [Result(out=json.dumps(report()))])
                vals.append(m["analyzer_accuracy"])
        self.assertEqual(vals[0], vals[1])

    def test_run_artifact_executes_cli_analyzer(self):
        c = case()
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [c])
            (m, _), inv = self.runmock(p, [Result(out=json.dumps(report()))])
        cmd = inv.call_args.args[0]
        self.assertEqual(
            cmd[:7],
            ["cargo", "run", "--quiet", "-p", "tailtriage-cli", "--", "analyze"],
        )
        self.assertNotIn("--allow-ambiguous-artifact", cmd)
        self.assertEqual(m["analyzer_execution"]["run_artifact_count"], 1)
        self.assertEqual(m["report_contract"]["case_count"], 0)

    def test_tracing_jsonl_executes_import_then_analyzer(self):
        c = case("trace", "tracing_span_jsonl")

        def invoke(cmd):
            if "import" in cmd:
                Path(cmd[-1]).write_text("{}")
                return Result()
            return Result(out=json.dumps(report()))

        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [c])
            with mock.patch.object(db, "_invoke", side_effect=invoke) as inv:
                m, f = db.run(p, 0, 0, 99)
        self.assertFalse(f)
        self.assertIn("import", inv.call_args_list[0].args[0])
        self.assertIn("tracing-spans-jsonl", inv.call_args_list[0].args[0])
        self.assertIn("analyze", inv.call_args_list[1].args[0])
        self.assertEqual(m["analyzer_execution"]["tracing_jsonl_count"], 1)

    def test_equivalent_encodings_share_one_accuracy_observation(self):
        a = case("a")
        b = case("b", observation_id="a")
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [a, b])
            (m, f), _ = self.runmock(
                p, [Result(out=json.dumps(report())), Result(out=json.dumps(report()))]
            )
        self.assertFalse(f)
        self.assertEqual(m["analyzer_execution"]["case_count"], 2)
        self.assertEqual(m["analyzer_accuracy"]["encoding_count"], 2)
        self.assertEqual(m["analyzer_accuracy"]["observation_count"], 1)

    def test_equivalent_encoding_diagnosis_disagreement_fails_observation(self):
        a = case("a")
        b = case("b", observation_id="a")
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [a, b])
            (m, f), _ = self.runmock(
                p,
                [
                    Result(out=json.dumps(report())),
                    Result(out=json.dumps(report(secondary="blocking_pool_pressure"))),
                ],
            )
        self.assertTrue(f)
        x = m["failed_analyzer_cases"][-1]
        self.assertEqual(x["observation_id"], "a")
        self.assertEqual(x["member_case_ids"], ["a", "b"])
        self.assertEqual(m["analyzer_accuracy"]["observation_count"], 0)

    def test_equivalent_encoding_confidence_disagreement_fails_observation(self):
        a = case("a")
        b = case("b", observation_id="a")
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [a, b])
            (m, _), _ = self.runmock(
                p,
                [
                    Result(out=json.dumps(report(conf="high"))),
                    Result(out=json.dumps(report(conf="medium"))),
                ],
            )
        self.assertIn(
            "confidence disagreement", m["failed_analyzer_cases"][-1]["error"]
        )

    def test_observation_labels_must_match(self):
        changes = {
            "ground_truth": "blocking_pool_pressure",
            "expected_primary_kinds": ["blocking_pool_pressure"],
            "required_visible_suspects": ["blocking_pool_pressure"],
            "exact_primary_kind": "blocking_pool_pressure",
        }
        for key, value in changes.items():
            with self.subTest(key=key):
                a = case("a")
                b = case("b", observation_id="a")
                b[key] = value
                with self.assertRaisesRegex(
                    ValueError,
                    "observation labels disagree|exact_primary_kind|ground_truth must be in",
                ):
                    db.validate_manifest(manifest(a, b))

    def test_accuracy_ground_truth_must_be_in_both_contract_lists(self):
        variants = [
            ({"expected_primary_kinds": ["blocking_pool_pressure"]}, False),
            ({"required_visible_suspects": ["blocking_pool_pressure"]}, False),
            ({}, True),
            (
                {
                    "expected_primary_kinds": [
                        "blocking_pool_pressure",
                        "application_queue_saturation",
                    ]
                },
                True,
            ),
            (
                {
                    "required_visible_suspects": [
                        "blocking_pool_pressure",
                        "application_queue_saturation",
                    ]
                },
                True,
            ),
        ]
        for changes, valid in variants:
            with self.subTest(changes=changes):
                c = case(**changes)
                if valid:
                    db.validate_manifest(manifest(c))
                else:
                    with self.assertRaisesRegex(ValueError, "ground_truth must be in"):
                        db.validate_manifest(manifest(c))

    def test_ground_truth_top1_cannot_be_high_confidence_wrong(self):
        c = case(
            expected_primary_kinds=[
                "blocking_pool_pressure",
                "application_queue_saturation",
            ]
        )
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [c])
            (m, _), _ = self.runmock(p, [Result(out=json.dumps(report()))])
        self.assertEqual(m["analyzer_accuracy"]["high_confidence_wrong_count"], 0)

    def test_class_type_and_eligibility_matrix(self):
        bad = [
            case("x", "analysis_report", False, validation_class="analyzer_execution"),
            case(
                "x",
                "synthetic_analysis_report",
                False,
                validation_class="analyzer_execution",
            ),
            case(validation_class="report_contract"),
            case("x", "tracing_span_jsonl", True, validation_class="report_contract"),
            case("x", "analysis_report", False, accuracy_eligible=True),
            case("x", "analysis_report", False, ground_truth="insufficient_evidence"),
            case("x", "analysis_report", False, observation_id="x"),
            case(observation_id=None),
            case(ground_truth=None),
            case(eligible=False, ground_truth="insufficient_evidence"),
            case(
                eligible=False,
                execution_expectation="failure",
                failure_stage="analyze",
                expected_error_substrings=[],
                forbidden_error_substrings=[],
                stdout_expectation="empty",
                accuracy_eligible=True,
            ),
        ]
        for c in bad:
            with self.subTest(c=c):
                with self.assertRaises(ValueError):
                    db.validate_manifest(manifest(c))

    def test_version_1_fields_are_rejected(self):
        for key in db.OLD:
            with self.assertRaisesRegex(ValueError, "version-1"):
                db.validate_manifest(manifest(case(**{key: []})))

    def test_expected_execution_failure_is_checked_not_skipped(self):
        base = case(
            eligible=False,
            execution_expectation="failure",
            failure_stage="analyze",
            expected_error_substrings=["needed"],
            forbidden_error_substrings=["secret"],
            stdout_expectation="empty",
        )
        scenarios = [
            (Result(1, "", "needed"), True),
            (Result(0, json.dumps(report()), ""), False),
            (Result(1, "", "missing"), False),
            (Result(1, "", "needed secret"), False),
            (Result(1, "output", "needed"), False),
        ]
        for result, passed in scenarios:
            with tempfile.TemporaryDirectory() as td:
                p = self.write(td, [base])
                (m, _), _ = self.runmock(p, [result])
                self.assertEqual(m["analyzer_execution"]["cases"][0]["passed"], passed)
        trace = case(
            "t",
            "tracing_span_jsonl",
            False,
            execution_expectation="failure",
            failure_stage="analyze",
            expected_error_substrings=[],
            forbidden_error_substrings=[],
            stdout_expectation="ignore",
        )
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [trace])
            (m, _), _ = self.runmock(p, [Result(1, "", "bad")])
            self.assertFalse(m["analyzer_execution"]["cases"][0]["passed"])

    def test_expected_failure_diagnostics_are_stderr_only(self):
        base = case(
            eligible=False,
            execution_expectation="failure",
            failure_stage="analyze",
            expected_error_substrings=["needed"],
            forbidden_error_substrings=["secret"],
            stdout_expectation="ignore",
        )
        scenarios = [
            (Result(1, "", "needed"), True),
            (Result(1, "needed", ""), False),
            (Result(1, "needed", ""), False),
            (Result(1, "", "needed secret"), False),
            (Result(1, "output", "needed"), False),
            (Result(1, "", "needed"), False),
            (Result(1, "anything", "needed"), True),
        ]
        policies = [
            "ignore",
            "non_empty",
            "ignore",
            "ignore",
            "empty",
            "non_empty",
            "ignore",
        ]
        for result, passed, policy in zip(
            (x[0] for x in scenarios), (x[1] for x in scenarios), policies
        ):
            with self.subTest(result=result.__dict__, policy=policy):
                c = dict(base, stdout_expectation=policy)
                with tempfile.TemporaryDirectory() as td:
                    p = self.write(td, [c])
                    (m, _), _ = self.runmock(p, [result])
                    row = m["analyzer_execution"]["cases"][0]
                self.assertEqual(row["passed"], passed)
                if result.stdout == "needed" and not result.stderr:
                    self.assertIn("missing stderr diagnostic", row["error"])

    def test_optional_contract_fields_are_strictly_validated(self):
        valid = {
            "exact_primary_kind": "application_queue_saturation",
            "max_primary_confidence": "high",
            "expected_evidence_quality": "strong",
            "expected_signal_statuses": {"queues": "present"},
            "must_include_confidence_notes": [],
            "must_include_route_warning": [],
            "must_include_temporal_warning": [],
            "expected_top_level_warnings": [],
            "expected_route_breakdowns": "empty",
            "expected_temporal_segments": "non_empty",
        }
        db.validate_manifest(manifest(case(**valid)))
        invalid = [
            ("exact_primary_kind", 42),
            ("exact_primary_kind", "unknown"),
            ("max_primary_confidence", 42),
            ("max_primary_confidence", "very_high"),
            ("expected_evidence_quality", 42),
            ("expected_evidence_quality", "complete"),
            ("expected_signal_statuses", []),
            ("expected_signal_statuses", {"unknown": "present"}),
            ("expected_signal_statuses", {"queues": "available"}),
            ("must_include_confidence_notes", "not-a-list"),
            ("must_include_route_warning", "not-a-list"),
            ("must_include_temporal_warning", [42]),
            ("expected_top_level_warnings", ["*"]),
            ("must_include_route_warning", ["*"]),
            ("must_include_temporal_warning", ["*"]),
            ("expected_route_breakdowns", "sometimes"),
            ("expected_route_breakdowns", True),
            ("expected_temporal_segments", "sometimes"),
            ("expected_temporal_segments", True),
        ]
        for field, value in invalid:
            with self.subTest(field=field, value=value):
                with self.assertRaises(ValueError):
                    db.validate_manifest(manifest(case(**{field: value})))
        for field in db.FAILURE_FIELDS:
            with self.subTest(success_field=field):
                with self.assertRaisesRegex(ValueError, "only on failure"):
                    db.validate_manifest(
                        manifest(
                            case(**{field: [] if "substrings" in field else "analyze"})
                        )
                    )

    def test_extract_rejects_malformed_report_shapes(self):
        mutations = [
            lambda r: r.update(secondary_suspects=[42]),
            lambda r: r.update(secondary_suspects=[{"kind": "unknown"}]),
            lambda r: r.update(
                secondary_suspects=[
                    {"kind": "blocking_pool_pressure", "confidence": "certain"}
                ]
            ),
            lambda r: r["primary_suspect"].update(score="one"),
            lambda r: r["primary_suspect"].update(score=True),
            lambda r: r.update(
                secondary_suspects=[{"kind": "blocking_pool_pressure", "score": "one"}]
            ),
            lambda r: r.update(
                secondary_suspects=[{"kind": "blocking_pool_pressure", "score": False}]
            ),
            lambda r: r["primary_suspect"].update(evidence=["ok", 42]),
            lambda r: r.update(
                secondary_suspects=[
                    {"kind": "blocking_pool_pressure", "evidence": [42]}
                ]
            ),
            lambda r: r["primary_suspect"].update(next_checks=[42]),
            lambda r: r.update(
                secondary_suspects=[
                    {"kind": "blocking_pool_pressure", "next_checks": [42]}
                ]
            ),
            lambda r: r["primary_suspect"].update(confidence_notes=[42]),
            lambda r: r.update(warnings=[42]),
            lambda r: r.update(evidence_quality=[]),
            lambda r: r.update(route_breakdowns={}),
            lambda r: r.update(route_breakdowns=[42]),
            lambda r: r.update(temporal_segments={}),
            lambda r: r.update(temporal_segments=[42]),
            lambda r: r.update(route_breakdowns=[{"warnings": [42]}]),
            lambda r: r.update(temporal_segments=[{"warnings": [42]}]),
        ]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                r = report()
                mutate(r)
                with self.assertRaises(ValueError):
                    db.extract(r)
        r = report(secondary="blocking_pool_pressure")
        r.update(
            evidence_quality={"quality": "strong"},
            route_breakdowns=[{"warnings": ["route"]}],
            temporal_segments=[{"warnings": ["time"]}],
        )
        r["primary_suspect"]["confidence_notes"] = ["note"]
        db.extract(r)

    def test_incomplete_equivalent_encoding_group_is_not_scored(self):
        bad_outputs = [
            Result(1, "", "boom"),
            Result(out="not json"),
            Result(out=json.dumps({"primary_suspect": {}})),
        ]
        for bad in bad_outputs:
            with self.subTest(bad=bad.__dict__):
                a = case("a")
                b = case("b", observation_id="a")
                with tempfile.TemporaryDirectory() as td:
                    p = self.write(td, [a, b])
                    (m, _), _ = self.runmock(p, [Result(out=json.dumps(report())), bad])
                acc = m["analyzer_accuracy"]
                self.assertEqual(acc["encoding_count"], 1)
                self.assertEqual(acc["observation_count"], 0)
                self.assertEqual(acc["per_ground_truth_counts"], {})
                self.assertEqual(acc["confusion_matrix"], {})
                self.assertEqual(acc["confidence_bucket_accuracy"], {})
                self.assertEqual(acc["observations"], [])
                row = m["failed_analyzer_cases"][-1]
                self.assertEqual(row["expected_member_ids"], ["a", "b"])
                self.assertEqual(row["usable_member_ids"], ["a"])
                self.assertEqual(row["unusable_member_ids"], ["b"])
                self.assertIn("incomplete", row["error"])
        a = case("a")
        b = case("b", observation_id="a")
        c = case("c")
        with tempfile.TemporaryDirectory() as td:
            p = self.write(td, [a, b, c])
            (m, _), _ = self.runmock(
                p,
                [
                    Result(out=json.dumps(report())),
                    Result(1, "", "boom"),
                    Result(out=json.dumps(report())),
                ],
            )
        self.assertEqual(m["analyzer_accuracy"]["encoding_count"], 2)
        self.assertEqual(m["analyzer_accuracy"]["observation_count"], 1)
        self.assertEqual(
            m["analyzer_accuracy"]["observations"][0]["observation_id"], "c"
        )

    def test_artifact_policy_is_typed(self):
        db.validate_manifest(manifest(case()))
        db.validate_manifest(manifest(case(artifact_policy="allow_ambiguous")))
        self.assertIn(
            "--allow-ambiguous-artifact",
            db._command("analyze", case(artifact_policy="allow_ambiguous"), Path("x")),
        )
        for c in [
            case("t", "tracing_span_jsonl", True, artifact_policy="strict"),
            case("r", "analysis_report", False, artifact_policy="strict"),
            case(artifact_policy="other"),
            case(command=["evil"]),
        ]:
            with self.assertRaises(ValueError):
                db.validate_manifest(manifest(c))

    def test_committed_manifest_invariants(self):
        p = Path(__file__).parents[2] / "validation/diagnostics/manifest.json"
        m = json.loads(p.read_text())
        db.validate_manifest(m)
        self.assertEqual(m["schema_version"], 2)
        self.assertTrue(
            any(c["validation_class"] == "analyzer_execution" for c in m["cases"])
        )
        self.assertTrue(
            any(c["validation_class"] == "report_contract" for c in m["cases"])
        )
        self.assertTrue(any(c["accuracy_eligible"] for c in m["cases"]))


if __name__ == "__main__":
    unittest.main()
