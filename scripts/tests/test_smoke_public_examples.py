"""Focused tests for selected public Markdown example extraction."""

import unittest

from scripts.smoke_public_examples import extract_markdown_rust_fence


class ExtractMarkdownRustFenceTests(unittest.TestCase):
    # TT-TEST: support
    def test_selects_exact_rust_no_run_fence_and_ignores_other_languages(self) -> None:
        markdown = """# Page
## Selected
```bash
echo ignored
```
```rust,no_run
fn main() {}
```
## Later
```rust,no_run
compile_error!("not selected");
```
"""
        self.assertEqual(
            extract_markdown_rust_fence(markdown, anchor="## Selected"),
            "fn main() {}\n",
        )

    # TT-TEST: support
    def test_missing_anchor_fails(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing Markdown anchor"):
            extract_markdown_rust_fence("# Page\n", anchor="## Missing")

    # TT-TEST: support
    def test_missing_selected_fence_fails(self) -> None:
        with self.assertRaisesRegex(ValueError, "found 0"):
            extract_markdown_rust_fence("## Selected\ntext\n", anchor="## Selected")

    # TT-TEST: support
    def test_ambiguous_selected_fence_fails(self) -> None:
        markdown = """## Selected
```rust,no_run
fn one() {}
```
```rust,no_run
fn two() {}
```
"""
        with self.assertRaisesRegex(ValueError, "found 2"):
            extract_markdown_rust_fence(markdown, anchor="## Selected")


if __name__ == "__main__":
    unittest.main()
