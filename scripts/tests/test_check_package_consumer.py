from __future__ import annotations

import unittest
from pathlib import Path

from scripts import check_package_consumer


class CheckPackageConsumerTests(unittest.TestCase):
    # TT-TEST: support
    def test_product_package_versions_requires_all_eight_packages(self) -> None:
        packages = [
            {"id": f"id-{name}", "name": name, "version": f"1.2.{index}"}
            for index, name in enumerate(check_package_consumer.PRODUCT_PACKAGES)
        ]
        packages.append({"id": "other", "name": "unrelated", "version": "9.9.9"})
        metadata = {
            "workspace_members": [package["id"] for package in packages],
            "packages": packages,
        }
        self.assertEqual(
            {
                name: f"1.2.{index}"
                for index, name in enumerate(check_package_consumer.PRODUCT_PACKAGES)
            },
            check_package_consumer.product_package_versions(metadata),
        )
        metadata["packages"] = packages[:-2] + packages[-1:]
        with self.assertRaisesRegex(ValueError, "missing product packages: tailtriage-cli"):
            check_package_consumer.product_package_versions(metadata)

    # TT-TEST: support
    def test_package_command_is_locked_offline_no_verify_and_dirty_is_opt_in(self) -> None:
        metadata = check_package_consumer.metadata_command()
        self.assertEqual(["cargo", "metadata"], metadata[:2])
        format_version = metadata.index("--format-version")
        self.assertEqual("1", metadata[format_version + 1])
        self.assertIn("--locked", metadata)
        self.assertIn("--offline", metadata)
        self.assertNotIn("publish", metadata)
        for network_or_login_argument in ("--registry", "login", "--token"):
            self.assertNotIn(network_or_login_argument, metadata)

        command = check_package_consumer.package_command()
        self.assertEqual(["cargo", "package"], command[:2])
        self.assertTrue({"--locked", "--offline", "--no-verify"}.issubset(command))
        self.assertNotIn("--allow-dirty", command)
        self.assertNotIn("publish", command)
        selected = [command[index + 1] for index, value in enumerate(command) if value == "-p"]
        self.assertEqual(list(check_package_consumer.PRODUCT_PACKAGES), selected)
        dirty = check_package_consumer.package_command(True)
        self.assertIn("--allow-dirty", dirty)
        self.assertNotIn("publish", dirty)

    # TT-TEST: support
    def test_consumer_manifest_uses_only_extracted_package_material(self) -> None:
        extracted_root = Path("/temporary/proof/extracted")
        extracted = {name: extracted_root / name for name in check_package_consumer.PRODUCT_PACKAGES}
        manifest = check_package_consumer.consumer_manifest(extracted)
        self.assertIn(
            f'tailtriage = {{ path = "{extracted["tailtriage"]}", default-features = false, features = ["full"] }}',
            manifest,
        )
        self.assertIn(f'tailtriage-analyzer = {{ path = "{extracted["tailtriage-analyzer"]}" }}', manifest)
        for name in check_package_consumer.PATCH_PACKAGES:
            self.assertIn(f'{name} = {{ path = "{extracted[name]}" }}', manifest)
        self.assertNotIn(str(check_package_consumer.REPO_ROOT), manifest)
        self.assertNotIn("0.1.1", manifest)

    # TT-TEST: support
    def test_consumer_source_exercises_facade_and_analyzer_public_usage(self) -> None:
        source = check_package_consumer.consumer_source()
        for usage in (
            "Tailtriage::builder",
            "begin_request",
            "finish_ok",
            "AnalyzeOptions::default",
            "shutdown",
        ):
            self.assertIn(usage, source)


if __name__ == "__main__":
    unittest.main()
