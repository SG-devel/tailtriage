#!/usr/bin/env python3
"""Tests for no-network runtime guard semantics.

The issue requires a verification path that *fails on runtime use* of selected
browser APIs, not merely checking for API existence.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

from no_network_runtime_guard import (
    BLOCKED_BROWSER_APIS,
    NetworkRuntimeViolation,
    install_no_network_runtime_guard,
)


class NoNetworkRuntimeGuardTests(unittest.TestCase):
    def test_install_overrides_all_blocked_apis(self) -> None:
        global_scope = {
            "fetch": object(),
            "XMLHttpRequest": object(),
            "WebSocket": object(),
            "EventSource": object(),
            "sendBeacon": object(),
        }

        install_no_network_runtime_guard(global_scope)

        for api_name in BLOCKED_BROWSER_APIS:
            with self.subTest(api_name=api_name):
                self.assertIn(api_name, global_scope)
                self.assertTrue(callable(global_scope[api_name]))

    def test_runtime_calls_fail_for_all_blocked_apis(self) -> None:
        global_scope: dict[str, object] = {}
        install_no_network_runtime_guard(global_scope)

        for api_name in BLOCKED_BROWSER_APIS:
            with self.subTest(api_name=api_name):
                with self.assertRaisesRegex(
                    NetworkRuntimeViolation,
                    rf"{api_name}",
                ):
                    # Runtime invocation must fail explicitly.
                    global_scope[api_name]()


if __name__ == "__main__":
    unittest.main()
