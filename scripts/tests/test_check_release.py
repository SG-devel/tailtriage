from __future__ import annotations

import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

from scripts import check_release


def result(returncode: int = 0, stdout: str = "", stderr: str = ""):
    return check_release.subprocess.CompletedProcess([], returncode, stdout, stderr)


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
    # TT-TEST: Z01 primary
    def test_classification_and_deterministic_dependency_order(self) -> None:
        packages = metadata()["packages"]
        publishable = [package for package in packages if check_release.is_publishable(package)]
        order, errors = check_release.publication_order(publishable, "1.2.3")
        self.assertEqual(["core", "private", "api", "cli"], order)
        self.assertEqual([], errors)
        self.assertEqual(["demo"], [package["name"] for package in packages if not check_release.is_publishable(package)])

    # TT-TEST: Z01 primary
    def test_readiness_failure_suppresses_package_and_publish_commands(self) -> None:
        calls: list[list[str]] = []

        def run(argv: list[str]):
            calls.append(argv)
            if argv[:2] == ["git", "status"]:
                return result(stdout=" M some/file\n?? another/file\n")
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
        self.assertIn("worktree is not clean:\n M some/file\n?? another/file", output.getvalue())
        self.assertNotIn("cargo publish", output.getvalue())

    # TT-TEST: Z01 primary
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
        self.assertEqual(
            [["cargo", "package", "--locked", "-p", "core", "-p", "private", "-p", "api", "-p", "cli"]], packages
        )
        printed = output.getvalue()
        self.assertLess(printed.index("cargo publish --locked -p core"), printed.index("cargo publish --locked -p api"))
        self.assertIn("cargo publish --locked -p private", printed)
        self.assertLess(printed.index("cargo publish --locked -p api"), printed.index("cargo publish --locked -p cli"))

    # TT-TEST: Z01 primary
    def test_successful_preflight_executes_checks_and_package_but_never_publish(self) -> None:
        calls: list[list[str]] = []

        def run(argv: list[str]):
            calls.append(argv)
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

        self.assertEqual(
            [
                ["git", "status", "--porcelain"],
                ["git", "rev-parse", "HEAD"],
                ["cargo", "metadata", "--format-version", "1", "--locked"],
                ["cargo", "package", "--locked", "-p", "core", "-p", "private", "-p", "api", "-p", "cli"],
            ],
            calls,
            "successful preflight must execute only readiness checks and packaging",
        )
        self.assertFalse(
            any(call[:2] == ["cargo", "publish"] for call in calls),
            f"cargo publish must remain an inert manual instruction; captured calls: {calls!r}",
        )
        self.assertIn(
            "Manual publication instructions (do not run automatically):",
            output.getvalue(),
        )
        self.assertIn("cargo publish --locked -p core", output.getvalue())

    # TT-TEST: Z01 primary
    def test_packaging_failure_suppresses_publication_commands(self) -> None:
        def run(argv: list[str]):
            if argv[:2] == ["git", "status"]:
                return result()
            if argv[:2] == ["git", "rev-parse"]:
                return result(stdout="abc\n")
            if argv[:2] == ["cargo", "metadata"]:
                return result(stdout=json.dumps(metadata()))
            return result(returncode=2, stdout="package stdout\n", stderr="package stderr\n")

        with tempfile.TemporaryDirectory() as directory:
            changelog = Path(directory) / "CHANGELOG.md"
            changelog.write_text("## [1.2.3] - 2026-08-07\n\nNotes.\n", encoding="utf-8")
            output = io.StringIO()
            with patch.object(check_release, "command", side_effect=run), redirect_stdout(output), redirect_stderr(output):
                self.assertEqual(1, check_release.check("1.2.3", changelog))
        printed = output.getvalue()
        self.assertIn(
            "command ['cargo', 'package', '--locked', '-p', 'core', '-p', 'private', '-p', 'api', '-p', 'cli'] exited with 2",
            printed,
        )
        self.assertIn("stdout:\npackage stdout", printed)
        self.assertIn("stderr:\npackage stderr", printed)
        self.assertNotIn("cargo publish", printed)


if __name__ == "__main__":
    unittest.main()
