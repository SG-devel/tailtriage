import tempfile
import unittest
from pathlib import Path

from scripts.validate_invariant_proofs import ValidationError, validate_repository


HEADER = "| ID | Invariant / contract | Behavior owner | Primary proof boundary | Secondary boundary / non-claim | Proof class / cadence |\n| --- | --- | --- | --- | --- | --- |\n"


class InvariantProofValidatorTests(unittest.TestCase):
    def root(self, rows=("A01",), rust="", python=""):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup); root = Path(temp.name)
        docs = root / "docs/dev"; docs.mkdir(parents=True)
        table = "# Registry\n\n## Invariant registry\n\n" + HEADER
        for invariant in (*rows, "P03"):
            table += f"| {invariant} | contract | owner | primary | secondary | unit |\n"
        (docs / "INVARIANT_PROOF_MATRIX.md").write_text(table)
        if rust: (root / "proof.rs").write_text(rust)
        if python: (root / "proof.py").write_text(python)
        return root

    def rejected(self, kind, **kwargs):
        with self.assertRaisesRegex(ValidationError, rf"\[{kind}\]"):
            validate_repository(self.root(**kwargs))

    # TT-TEST: support
    def test_valid_rust_primary(self):
        self.assertEqual(validate_repository(self.root(rust="// TT-TEST: A01 primary\n#[test]\nfn proof() {}\n")).rust_tests, 1)

    # TT-TEST: support
    def test_valid_rust_secondary_and_primary(self):
        summary = validate_repository(self.root(rust="// TT-TEST: A01 primary\n// TT-TEST: P03 secondary\n#[test]\nfn proof() {}\n")); self.assertEqual(summary.secondary_markers, 1)

    # TT-TEST: support
    def test_valid_rust_support(self):
        summary = validate_repository(self.root(rust="// TT-TEST: A01 primary\n#[test]\nfn proof() {}\n// TT-TEST: support\n#[test]\nfn helper() {}\n")); self.assertEqual(summary.support_tests, 1)

    # TT-TEST: support
    def test_tokio_forms_and_cfg_attribute(self):
        rust="// TT-TEST: A01 primary\n#[cfg(unix)]\n#[test]\nfn one() {}\n// TT-TEST: support\n#[tokio::test]\nasync fn two() {}\n// TT-TEST: support\n#[tokio::test(flavor = \"current_thread\")]\nasync fn three() {}\n"; self.assertEqual(validate_repository(self.root(rust=rust)).rust_tests, 3)

    # TT-TEST: support
    def test_missing_rust_marker_rejected(self):
        self.rejected("missing-classification", rust="#[test]\nfn proof() {}\n")

    # TT-TEST: support
    def test_orphan_and_module_markers_rejected(self):
        self.rejected("orphan-marker", rust="// TT-TEST: A01 primary\nmod tests {}\n")

    # TT-TEST: support
    def test_marker_separated_by_item_rejected(self):
        self.rejected("missing-classification", rust="// TT-TEST: A01 primary\nconst X: u8 = 1;\n#[test]\nfn proof() {}\n")

    # TT-TEST: support
    def test_unknown_and_malformed_rejected(self):
        self.rejected("unknown-id", rust="// TT-TEST: B01 primary\n#[test]\nfn proof() {}\n")
        malformed = (
            "// TT-TEST: A01 PRIMARY",
            "//TT-TEST: A01 primary",
            "//  TT-TEST: A01 primary",
            "// TT-TEST:  A01 primary",
            "// TT-TEST: A01  primary",
            "// TT-TEST: A01 primary ",
        )
        for marker in malformed:
            with self.subTest(marker=marker):
                self.rejected("malformed-marker", rust=f"{marker}\n#[test]\nfn proof() {{}}\n")
        for marker in (
            "#TT-TEST: A01 primary",
            "#  TT-TEST: A01 primary",
            "# TT-TEST:  A01 primary",
        ):
            with self.subTest(marker=marker):
                self.rejected("malformed-marker", python=f"{marker}\ndef test_proof(): pass\n")

    # TT-TEST: support
    def test_duplicate_conflict_and_support_mix_rejected(self):
        self.rejected("duplicate-marker", rust="// TT-TEST: A01 primary\n// TT-TEST: A01 primary\n#[test]\nfn proof() {}\n"); self.rejected("conflicting-marker", rust="// TT-TEST: A01 primary\n// TT-TEST: A01 secondary\n#[test]\nfn proof() {}\n"); self.rejected("support-mix", rust="// TT-TEST: A01 primary\n// TT-TEST: support\n#[test]\nfn proof() {}\n")

    # TT-TEST: support
    def test_multiple_invariants_on_one_test(self):
        summary=validate_repository(self.root(rows=("A01","A02"),rust="// TT-TEST: A01 primary\n// TT-TEST: A02 primary\n#[test]\nfn proof() {}\n")); self.assertEqual(summary.primary_markers,2)

    # TT-TEST: support
    def test_python_primary_secondary_support_and_top_level(self):
        py="# TT-TEST: A01 primary\n@decorator(\n    \"value\",\n)\ndef test_one(): pass\nclass C:\n # TT-TEST: P03 secondary\n def test_two(self): pass\n # TT-TEST: support\n async def test_three(self): pass\n"; summary = validate_repository(self.root(python=py)); self.assertEqual((summary.python_tests, summary.primary_markers, summary.secondary_markers, summary.support_tests), (3, 1, 1, 1))

    # TT-TEST: support
    def test_python_method_missing_and_class_marker_rejected(self):
        self.rejected("missing-classification", python="class C:\n def test_one(self): pass\n"); self.rejected("orphan-marker", python="# TT-TEST: A01 primary\nclass C: pass\n")

    # TT-TEST: support
    def test_python_string_ignored_and_orphan_rejected(self):
        summary = validate_repository(self.root(python='x="# TT-TEST: B01 primary"\n# TT-TEST: A01 primary\ndef test_real(): pass\n'))
        self.assertEqual((summary.python_tests, summary.primary_markers), (1, 1))
        self.rejected("orphan-marker", python="# TT-TEST: A01 primary\nx=1\n")

    # TT-TEST: support
    def test_rust_raw_string_marker_is_ignored(self):
        rust='const X: &str = r#"// TT-TEST: B01 primary\n#[test]\nfn fake() {}"#;\n// TT-TEST: A01 primary\n#[test]\nfn real() {}\n'; self.assertEqual(validate_repository(self.root(rust=rust)).rust_tests,1)

    # TT-TEST: support
    def test_unsupported_inline_rust_test_declaration_rejected(self):
        self.rejected("unsupported-test-form", rust="// TT-TEST: A01 primary\n#[test] fn proof() {}\n")

    # TT-TEST: support
    def test_primary_coverage_rules(self):
        self.rejected("missing-primary", rust="// TT-TEST: A01 secondary\n#[test]\nfn proof() {}\n")

    # TT-TEST: support
    def test_duplicate_registry_heading_rejected(self):
        root=self.root(rust="// TT-TEST: A01 primary\n#[test]\nfn proof() {}\n"); p=root/"docs/dev/INVARIANT_PROOF_MATRIX.md"; p.write_text(p.read_text()+"\n## Invariant registry\n"); self.assertRaisesRegex(ValidationError,"\\[registry\\]",validate_repository,root)

    # TT-TEST: support
    def test_malformed_registry_separator_rejected(self):
        root=self.root(); p=root/"docs/dev/INVARIANT_PROOF_MATRIX.md"; p.write_text(p.read_text().replace("| --- | --- | --- | --- | --- | --- |","| -- | --- | --- | --- | --- | --- |")); self.assertRaisesRegex(ValidationError,"\\[registry\\]",validate_repository,root)

    # TT-TEST: support
    def test_empty_registry_cell_rejected(self):
        root=self.root(); p=root/"docs/dev/INVARIANT_PROOF_MATRIX.md"; p.write_text(p.read_text().replace("| A01 | contract |","| A01 |  |")); self.assertRaisesRegex(ValidationError,"\\[registry\\]",validate_repository,root)

    # TT-TEST: support
    def test_malformed_registry_cell_count_rejected(self):
        root=self.root(); p=root/"docs/dev/INVARIANT_PROOF_MATRIX.md"; p.write_text(p.read_text().replace("| A01 | contract | owner | primary | secondary | unit |","| A01 | contract | owner | primary | unit |")); self.assertRaisesRegex(ValidationError,"\\[registry\\]",validate_repository,root)

    # TT-TEST: support
    def test_duplicate_registry_id_rejected(self):
        root=self.root(); p=root/"docs/dev/INVARIANT_PROOF_MATRIX.md"; p.write_text(p.read_text()+"| A01 | c | o | p | s | u |\n"); self.assertRaisesRegex(ValidationError,"\\[registry\\]",validate_repository,root)

    # TT-TEST: support
    def test_command_primary_rejected_secondary_accepted(self):
        self.rejected("command-owned-primary", rust="// TT-TEST: A01 primary\n// TT-TEST: P03 primary\n#[test]\nfn proof() {}\n"); validate_repository(self.root(rust="// TT-TEST: A01 primary\n// TT-TEST: P03 secondary\n#[test]\nfn proof() {}\n"))

    # TT-TEST: support
    def test_legacy_marker_rejected(self):
        self.rejected("legacy-marker", rust="// TT-INVARIANT: A01 primary\n")

    # TT-TEST: support
    def test_current_repository_linkage_passes(self):
        summary=validate_repository(Path(__file__).resolve().parents[2]); self.assertEqual(summary.registry_invariants,72)
