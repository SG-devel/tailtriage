#!/usr/bin/env python3
"""Utilities to enforce no-network browser-runtime behavior in tests.

This module models a browser shell hardening strategy: selected network APIs are
replaced with guards that fail immediately when used.
"""

from __future__ import annotations

from collections.abc import MutableMapping
from typing import Any, Callable

BLOCKED_BROWSER_APIS = (
    "fetch",
    "XMLHttpRequest",
    "WebSocket",
    "EventSource",
    "sendBeacon",
)


class NetworkRuntimeViolation(RuntimeError):
    """Raised when a blocked browser network API is used at runtime."""


def _raise_network_violation(api_name: str) -> None:
    raise NetworkRuntimeViolation(
        f"Network API '{api_name}' is disabled in no-network runtime checks"
    )


def _make_guard(api_name: str) -> Callable[..., Any]:
    def _guard(*_args: Any, **_kwargs: Any) -> None:
        _raise_network_violation(api_name)

    return _guard


def install_no_network_runtime_guard(global_scope: MutableMapping[str, Any]) -> None:
    """Install guards for browser-style network APIs on a provided global scope.

    The scope argument is intentionally generic so tests can run without requiring
    an actual browser environment.
    """

    for api_name in BLOCKED_BROWSER_APIS:
        global_scope[api_name] = _make_guard(api_name)
