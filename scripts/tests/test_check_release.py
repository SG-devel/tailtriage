from __future__ import annotations

import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

from scripts import check_release


def result(returncode: int = 0, stdout: str = ""):
    return check_release.subprocess.CompletedProcess([], returncode, stdout, "")


def metadata(version: str = "1.2.3") -> dict:
    packages = [
        {"id": "core", "name": "core", "version": version, "publish": None, "dependencies": []},
        {
            "id": "api",
            "name": "api",
            "version": version,
            "publish": ["crates-io"],
            "dependencies": [{"name": "core", "rename": "foundation", "req": "^1.2.3", "kind": "build"}],
        },
        {"id": "cli", "name": "cli", "version": version, "publish": None, "dependencies": [{"name": "core", "req": "^1.2.3", "kind": None}]},
        {"id": "demo", "name": "demo", "version": version, "publish": [], "dependencies": []},
        {"id": "private", "name": "private", "version": version, "publish": ["internal"], "dependencies": []},
    ]
    return {"workspace_members": [package["id"] for package in packages], "packages": packages}


class CheckReleaseTests(unittest.TestCase):
    def test_classification_and_deterministic_dependency_order(self) -> None:
        packages = metadata()["packages"]
        publishable = [package for package in packages if check_release.is_publishable(package)]
        order, errors = check_release.publication_order(publishable, "1.2.3")
        self.assertEqual(["core", "api", "cli"], order)
        self.assertEqual([], errors)
        self.assertEqual(["demo", "private"], [package["name"] for package in packages if not check_release.is_publishable(package)])

    def test_readiness_failure_suppresses_package_and_publish_commands(self) -> None:
        calls: list[list[str]] = []

        def run(argv: list[str]):
            calls.append(argv)
            if argv[:2] == ["git", "status"]:
                return result(stdout="dirty\n")
            if argv[:2] == ["git", "rev-parse"]:
                return result(stdout="abc\n")
            return result(stdout=json.dumps(metadata()))

        with tempfile.TemporaryDirectory() as directory:
            changelog = Path(directory) / "CHANGELOG.md"
            changelog.write_text("## [1.2.3] - Unreleased\n\nNotes.\n", encoding="utf-8")
            output = io.StringIO()
            with patch.object(check_release, "command", side_effect=run), redirect_stdout(output), redirect_stderr(output):
                self.assertEqual(1, check_release.check("1.2.3", changelog))
        self.assertFalse(any(call[:2] == ["cargo", "package"] for call in calls))
        self.assertNotIn("cargo publish", output.getvalue())

    def test_success_packages_once_and_prints_publication_order(self) -> None:
        calls: list[list[str]] = []

        def run(argv: list[str]):
            calls.append(argv)
            if argv[:2] == ["git", "status"]:
                return result()
            if argv[:2] == ["git", "rev-parse"]:
                return result(stdout="abc\n")
            if argv[:2] == ["cargo", "metadata"]:
                return result(stdout=json.dumps(metadata()))
            return result()

        with tempfile.TemporaryDirectory() as directory:
            changelog = Path(directory) / "CHANGELOG.md"
            changelog.write_text("## [1.2.3] - 2026-08-07\n\nNotes.\n", encoding="utf-8")
            output = io.StringIO()
            with patch.object(check_release, "command", side_effect=run), redirect_stdout(output):
                self.assertEqual(0, check_release.check("1.2.3", changelog))
        packages = [call for call in calls if call[:2] == ["cargo", "package"]]
        self.assertEqual([["cargo", "package", "--locked", "-p", "core", "-p", "api", "-p", "cli"]], packages)
        printed = output.getvalue()
        self.assertLess(printed.index("cargo publish --locked -p core"), printed.index("cargo publish --locked -p api"))
        self.assertLess(printed.index("cargo publish --locked -p api"), printed.index("cargo publish --locked -p cli"))

    def test_packaging_failure_suppresses_publication_commands(self) -> None:
        def run(argv: list[str]):
            if argv[:2] == ["git", "status"]:
                return result()
            if argv[:2] == ["git", "rev-parse"]:
                return result(stdout="abc\n")
            if argv[:2] == ["cargo", "metadata"]:
                return result(stdout=json.dumps(metadata()))
            return result(returncode=2)

        with tempfile.TemporaryDirectory() as directory:
            changelog = Path(directory) / "CHANGELOG.md"
            changelog.write_text("## [1.2.3] - 2026-08-07\n\nNotes.\n", encoding="utf-8")
            output = io.StringIO()
            with patch.object(check_release, "command", side_effect=run), redirect_stdout(output), redirect_stderr(output):
                self.assertEqual(1, check_release.check("1.2.3", changelog))
        self.assertIn("cargo package exited with 2", output.getvalue())
        self.assertNotIn("cargo publish", output.getvalue())


if __name__ == "__main__":
    unittest.main()
