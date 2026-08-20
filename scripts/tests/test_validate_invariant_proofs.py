import tempfile
import unittest
from pathlib import Path
from scripts.validate_invariant_proofs import ValidationError, validate

HEADER = """# Test\n\n## Invariant registry\n\n| ID | Primary linkage | Invariant / contract | Behavior owner | Primary proof boundary | Secondary boundary / non-claim | Proof class / cadence |\n| --- | --- | --- | --- | --- | --- | --- |\n"""

class InvariantProofValidatorTests(unittest.TestCase):
    def check(self, rows=None, rust="", python=""):
        temporary=tempfile.TemporaryDirectory(); self.addCleanup(temporary.cleanup); root=Path(temporary.name)
        matrix=root/'docs/dev/INVARIANT_PROOF_MATRIX.md'; matrix.parent.mkdir(parents=True)
        matrix.write_text(HEADER+(rows or self.row()),encoding='utf-8')
        if rust: (root/'proof.rs').write_text(rust,encoding='utf-8')
        if python: (root/'proof.py').write_text(python,encoding='utf-8')
        return lambda: validate(root,matrix)
    def row(self, identifier='A01', linkage='test'):
        return f'| {identifier} | {linkage} | contract | owner | proof | boundary | unit CI |\n'
    # TT-TEST: support
    def test_valid_rust_primary(self): self.check(rust='// TT-TEST: A01 primary\n#[test]\nfn proof() {}\n')()
    # TT-TEST: support
    def test_valid_rust_secondary_and_primary(self): self.check(rust='// TT-TEST: A01 secondary\n#[test]\nfn edge() {}\n// TT-TEST: A01 primary\n#[test]\nfn proof() {}\n')()
    # TT-TEST: support
    def test_valid_rust_support(self): self.check(rows=self.row(linkage='command'),rust='// TT-TEST: support\n#[test]\nfn helper() {}\n')()
    # TT-TEST: support
    def test_tokio_forms_and_cfg_attribute(self):
        rust='// TT-TEST: A01 primary\n#[cfg(unix)]\n#[tokio::test]\nasync fn one() {}\n// TT-TEST: support\n#[tokio::test(flavor = "current_thread")]\nasync fn two() {}\n'
        self.check(rust=rust)()
    # TT-TEST: support
    def test_missing_rust_marker_rejected(self):
        with self.assertRaisesRegex(ValidationError,'test `proof` has no'): self.check(rust='#[test]\nfn proof() {}\n')()
    # TT-TEST: support
    def test_orphan_and_module_markers_rejected(self):
        for rust in ('// TT-TEST: A01 primary\nmod tests {}\n','// TT-TEST: A01 primary\nmod tests { #[test] fn proof() {} }\n'):
            with self.subTest(rust=rust), self.assertRaisesRegex(ValidationError,'not attached'): self.check(rust=rust)()
    # TT-TEST: support
    def test_marker_separated_by_item_rejected(self):
        with self.assertRaisesRegex(ValidationError,'not attached'): self.check(rust='// TT-TEST: A01 primary\nstruct X;\n#[test]\nfn proof() {}\n')()
    # TT-TEST: support
    def test_unknown_and_malformed_rejected(self):
        for marker,problem in [('B01 primary','unknown'),('A01 PRIMARY','malformed')]:
            with self.subTest(marker=marker), self.assertRaisesRegex(ValidationError,problem): self.check(rust=f'// TT-TEST: {marker}\n#[test]\nfn proof() {{}}\n')()
    # TT-TEST: support
    def test_duplicate_conflict_and_support_mix_rejected(self):
        cases=[('// TT-TEST: A01 primary\n// TT-TEST: A01 primary\n','duplicate'),('// TT-TEST: A01 primary\n// TT-TEST: A01 secondary\n','both primary'),('// TT-TEST: support\n// TT-TEST: A01 primary\n','mixes support')]
        for marks,problem in cases:
            with self.subTest(problem=problem), self.assertRaisesRegex(ValidationError,problem): self.check(rust=marks+'#[test]\nfn proof() {}\n')()
    # TT-TEST: support
    def test_multiple_invariants_on_one_test(self): self.check(rows=self.row()+self.row('B01'),rust='// TT-TEST: A01 primary\n// TT-TEST: B01 primary\n#[test]\nfn proof() {}\n')()
    # TT-TEST: support
    def test_python_primary_secondary_support_and_top_level(self):
        source='# TT-TEST: A01 primary\ndef test_one(): pass\n# TT-TEST: A01 secondary\ndef test_two(): pass\n# TT-TEST: support\ndef test_three(): pass\n'
        self.check(python=source)()
    # TT-TEST: support
    def test_python_method_missing_and_class_marker_rejected(self):
        for source in ('class T:\n    def test_one(self): pass\n','# TT-TEST: A01 primary\nclass T:\n    def test_one(self): pass\n'):
            with self.subTest(source=source), self.assertRaises(ValidationError): self.check(python=source)()
    # TT-TEST: support
    def test_python_string_ignored_and_orphan_rejected(self):
        with self.assertRaisesRegex(ValidationError,'missing primary'): self.check(python='value="# TT-TEST: A01 primary"\n')()
        with self.assertRaisesRegex(ValidationError,'not attached'): self.check(python='# TT-TEST: A01 primary\nvalue=1\n')()
    # TT-TEST: support
    def test_primary_coverage_rules(self):
        with self.assertRaisesRegex(ValidationError,'missing primary'): self.check(rust='// TT-TEST: A01 secondary\n#[test]\nfn edge() {}\n')()
        self.check(rust='// TT-TEST: A01 primary\n#[test]\nfn one() {}\n// TT-TEST: A01 primary\n#[test]\nfn two() {}\n')()
    # TT-TEST: support
    def test_command_primary_rejected_secondary_accepted(self):
        with self.assertRaisesRegex(ValidationError,'command invariant'): self.check(rows=self.row(linkage='command'),rust='// TT-TEST: A01 primary\n#[test]\nfn proof() {}\n')()
        self.check(rows=self.row(linkage='command'),rust='// TT-TEST: A01 secondary\n#[test]\nfn edge() {}\n')()
    # TT-TEST: support
    def test_legacy_marker_rejected(self):
        legacy='TT-'+'INVARIANT'
        with self.assertRaisesRegex(ValidationError,'legacy'): self.check(rows=self.row(linkage='command'),rust=f'// {legacy}: A01 primary\n')()
    # TT-TEST: support
    def test_current_repository_linkage_passes(self): validate(Path(__file__).resolve().parents[2])

if __name__ == '__main__': unittest.main()
