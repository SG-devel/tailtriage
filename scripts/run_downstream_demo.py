#!/usr/bin/env python3
"""Compatibility wrapper for downstream demo runs."""

from __future__ import annotations

import sys

from demo_tool import main


def build_argv(raw_args: list[str]) -> list[str]:
    """Translate wrapper args into ``demo_tool run downstream`` args.

    Backward compatibility:
    - ``run_downstream_demo.py`` runs the default downstream scenario.
    - ``run_downstream_demo.py <path>`` treats ``<path>`` as ``--artifact-path``.
    - Explicit flags are passed through unchanged.
    """
    if not raw_args:
        return ["run", "downstream"]

    if raw_args[0].startswith("-"):
        return ["run", "downstream", *raw_args]

    return ["run", "downstream", "--artifact-path", raw_args[0], *raw_args[1:]]


if __name__ == "__main__":
    main(build_argv(sys.argv[1:]))
