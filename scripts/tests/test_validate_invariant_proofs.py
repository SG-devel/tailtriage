import tempfile
import unittest
from pathlib import Path

from scripts.validate_invariant_proofs import ValidationError, validate


HEADER = """# Test\n\n## Invariant registry\n\n| ID | Primary linkage | Invariant / contract | Behavior owner | Primary proof boundary | Secondary boundary / non-claim | Proof class / cadence |\n| --- | --- | --- | --- | --- | --- | --- |\n"""


class InvariantProofValidatorTests(unittest.TestCase):
    def check(self, rows, rust="", python=""):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        matrix = root / "docs/dev/INVARIANT_PROOF_MATRIX.md"
        matrix.parent.mkdir(parents=True)
        matrix.write_text(HEADER + rows, encoding="utf-8")
        if rust:
            (root / "proof.rs").write_text(rust, encoding="utf-8")
        if python:
            (root / "proof.py").write_text(python, encoding="utf-8")
        return lambda: validate(root, matrix)

    def row(self, identifier="A01", linkage="marker"):
        return f"| {identifier} | {linkage} | contract | owner | proof | boundary | unit CI |\n"

    def test_valid_minimal_registry_and_markers(self):
        self.check(self.row(), rust="// " + "TT-INVARIANT: A01 primary\n")()

    def test_duplicate_registry_id(self):
        with self.assertRaisesRegex(ValidationError, "duplicate invariant ID"):
            self.check(self.row() + self.row(), rust="// " + "TT-INVARIANT: A01 primary\n")()

    def test_malformed_row_cell_count(self):
        with self.assertRaisesRegex(ValidationError, "expected 7"):
            self.check("| A01 | marker | too few |\n")()

    def test_invalid_linkage(self):
        with self.assertRaisesRegex(ValidationError, "invalid primary linkage"):
            self.check(self.row(linkage="manual"))()

    def test_malformed_rust_marker(self):
        with self.assertRaisesRegex(ValidationError, "proof.rs:1: malformed"):
            self.check(self.row(), rust="// " + "TT-INVARIANT: A01 PRIMARY\n")()

    def test_unknown_rust_marker_id(self):
        with self.assertRaisesRegex(ValidationError, "proof.rs:2: unknown"):
            self.check(self.row(), rust="// ordinary\n// " + "TT-INVARIANT: B01 primary\n")()

    def test_malformed_python_marker(self):
        with self.assertRaisesRegex(ValidationError, "proof.py:1: malformed"):
            self.check(self.row(), python="# " + "TT-INVARIANT A01 primary\n")()

    def test_unknown_python_marker_id(self):
        with self.assertRaisesRegex(ValidationError, "proof.py:1: unknown"):
            self.check(self.row(), python="# " + "TT-INVARIANT: B01 primary\n")()

    def test_python_marker_looking_string_is_ignored(self):
        source = 'value = "# TT-' + 'INVARIANT: B01 primary"\n# TT-' + 'INVARIANT: A01 primary\n'
        self.check(self.row(), python=source)()

    def test_marker_invariant_missing_primary(self):
        with self.assertRaisesRegex(ValidationError, "missing primary.*A01"):
            self.check(self.row(), rust="// " + "TT-INVARIANT: A01 secondary\n")()

    def test_multiple_primaries_accepted(self):
        self.check(self.row(), rust="// " + "TT-INVARIANT: A01 primary\n// TT-" + "INVARIANT: A01 primary\n")()

    def test_secondaries_optional(self):
        self.check(self.row(), rust="// " + "TT-INVARIANT: A01 primary\n")()

    def test_command_primary_rejected(self):
        with self.assertRaisesRegex(ValidationError, "command invariant"):
            self.check(self.row(linkage="command"), rust="// " + "TT-INVARIANT: A01 primary\n")()

    def test_command_secondary_accepted(self):
        self.check(self.row(linkage="command"), python="# " + "TT-INVARIANT: A01 secondary\n")()

    def test_current_repository_linkage_passes(self):
        root = Path(__file__).resolve().parents[2]
        validate(root)


if __name__ == "__main__":
    unittest.main()
